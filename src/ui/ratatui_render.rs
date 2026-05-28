//! ratatui 版本的牌桌渲染。
//!
//! 与 `render.rs` 的手写 crossterm 路径并存：当 `Layout::Ratatui` 时由
//! `render.rs` 中的分发函数转发到这里。
//!
//! 设计要点：
//! - `thread_local` 持有一个 `Terminal<CrosstermBackend>`；进出备用屏幕由
//!   `enter_screen` / `leave_screen` 控制。
//! - 同时缓存最近一次渲染快照，便于 `wait_for_continue` 等只追加 overlay
//!   的场景重绘整桌。
//! - 玩家行动菜单 / 加注子菜单作为 overlay 一并嵌入帧中绘制，避免与
//!   ratatui 的 diff 渲染冲突（不写裸 `print!`）。

use std::cell::RefCell;
use std::io::{self, Stdout};

use crossterm::event::{Event, KeyCode, read};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout as RLayout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use crate::card::{Card, Suit};
use crate::game::action::Action;
use crate::game::state::{HandState, PlayerStatus};
use crate::hand_eval::describe_hero;
use crate::ui::render::ContinueAction;

thread_local! {
    static TERM: RefCell<Option<Terminal<CrosstermBackend<Stdout>>>> = const { RefCell::new(None) };
    static LAST: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
}

/// 牌背配色：在黑色底上比 Color::Blue 更柔和，又保留一丝"卡片背面"暗示。
/// ANSI 256 索引 109 是一个中性的青灰色。
const CARD_BACK: Color = Color::Indexed(109);

/// 最近一次渲染所需的所有数据；用于 overlay 重绘。
#[derive(Clone)]
struct Snapshot {
    state: HandState,
    hero_seat: Option<usize>,
    log: Vec<String>,
    reveal_all: bool,
    winners: Vec<usize>,
    tilt_marks: Vec<bool>,
    hud: Option<Vec<String>>,
}

/// 各种叠加在主视图上的提示。
enum Overlay<'a> {
    None,
    Prompt(&'a str),
    Action {
        can_check: bool,
        to_call: u64,
    },
    ConfirmAllIn(u64),
    Raise {
        min_total: u64,
        max_total: u64,
        half_pot: u64,
        two_thirds: u64,
        pot_size: u64,
        over_pot: u64,
        buf: &'a str,
        err: Option<&'a str>,
    },
}

pub fn enter_screen() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let term = Terminal::new(CrosstermBackend::new(stdout))?;
    TERM.with(|c| *c.borrow_mut() = Some(term));
    Ok(())
}

pub fn leave_screen() -> io::Result<()> {
    let term_opt = TERM.with(|c| c.borrow_mut().take());
    let _ = disable_raw_mode();
    if let Some(mut t) = term_opt {
        let _ = execute!(t.backend_mut(), LeaveAlternateScreen);
        let _ = t.show_cursor();
    } else {
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
    LAST.with(|c| c.borrow_mut().take());
    Ok(())
}

/// 从 `RenderOptions` 接收并保存快照，然后绘制无 overlay 的帧。
pub fn render(state: &HandState, opts: super::render::RenderOptions<'_>) -> io::Result<()> {
    let snap = Snapshot {
        state: state.clone(),
        hero_seat: opts.hero_seat,
        log: opts.log.to_vec(),
        reveal_all: opts.reveal_all,
        winners: opts.winners.to_vec(),
        tilt_marks: opts.tilt_marks.to_vec(),
        hud: opts.hud.map(|h| h.to_vec()),
    };
    LAST.with(|c| *c.borrow_mut() = Some(snap.clone()));
    draw(&snap, Overlay::None)
}

pub fn render_showdown(
    state: &HandState,
    log: &[String],
    winners: &[usize],
    tilt_marks: &[bool],
    hud: Option<&[String]>,
) -> io::Result<()> {
    let snap = Snapshot {
        state: state.clone(),
        hero_seat: None,
        log: log.to_vec(),
        reveal_all: true,
        winners: winners.to_vec(),
        tilt_marks: tilt_marks.to_vec(),
        hud: hud.map(|h| h.to_vec()),
    };
    LAST.with(|c| *c.borrow_mut() = Some(snap.clone()));
    draw(&snap, Overlay::None)
}

/// 等待 space/Enter（next）或 R（replay）。q/Esc → Interrupted。
pub fn wait_for_continue(prompt: &str) -> io::Result<ContinueAction> {
    redraw_with(Overlay::Prompt(prompt))?;
    loop {
        if let Event::Key(k) = read()? {
            match k.code {
                KeyCode::Char(' ') | KeyCode::Enter => return Ok(ContinueAction::Next),
                KeyCode::Char('r') | KeyCode::Char('R') => return Ok(ContinueAction::Replay),
                KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "user quit"));
                }
                _ => {}
            }
        }
    }
}

/// 真人玩家行动菜单（ratatui 版本）。
pub fn prompt_action(state: &HandState, seat: usize) -> io::Result<Action> {
    let to_call = state.to_call_for(seat);
    let stack = state.players[seat].stack;
    let can_check = to_call == 0;

    loop {
        redraw_with(Overlay::Action {
            can_check,
            to_call: to_call.min(stack),
        })?;
        let Event::Key(k) = read()? else { continue };
        match k.code {
            KeyCode::Char('f') | KeyCode::Char('F') => return Ok(Action::Fold),
            KeyCode::Char('k') | KeyCode::Char('K') if can_check => return Ok(Action::Check),
            KeyCode::Char('c') | KeyCode::Char('C') if !can_check => return Ok(Action::Call),
            KeyCode::Char('a') | KeyCode::Char('A') => {
                if confirm_allin(state, seat)? {
                    return Ok(Action::AllIn);
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if let Some(total) = prompt_raise_amount(state, seat)? {
                    if state.current_bet == 0 {
                        return Ok(Action::Bet(total));
                    } else {
                        return Ok(Action::Raise(total));
                    }
                }
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "user quit"));
            }
            _ => {}
        }
    }
}

fn confirm_allin(state: &HandState, seat: usize) -> io::Result<bool> {
    let total = state.players[seat].committed_round + state.players[seat].stack;
    redraw_with(Overlay::ConfirmAllIn(total))?;
    loop {
        if let Event::Key(k) = read()? {
            return Ok(matches!(k.code, KeyCode::Char('a') | KeyCode::Char('A')));
        }
    }
}

fn prompt_raise_amount(state: &HandState, seat: usize) -> io::Result<Option<u64>> {
    let min_total = if state.current_bet == 0 {
        state.bb
    } else {
        state.current_bet + state.min_raise
    };
    let cur_round = state.players[seat].committed_round;
    let max_total = cur_round + state.players[seat].stack;
    let to_call = state.current_bet.saturating_sub(cur_round);
    let pot_after_call = state.total_pot() + to_call;
    let cur_bet = state.current_bet;
    let bb = state.bb;
    let preset = |target: u64| target.clamp(min_total, max_total);
    let half_pot = preset(cur_bet.max(bb) + pot_after_call / 2);
    let two_thirds = preset(cur_bet.max(bb) + pot_after_call * 2 / 3);
    let pot_size = preset(cur_bet.max(bb) + pot_after_call);
    let over_pot = preset(cur_bet.max(bb) + pot_after_call * 2);

    let mut buf = String::new();
    let mut err: Option<String> = None;
    loop {
        redraw_with(Overlay::Raise {
            min_total,
            max_total,
            half_pot,
            two_thirds,
            pot_size,
            over_pot,
            buf: &buf,
            err: err.as_deref(),
        })?;
        let Event::Key(k) = read()? else { continue };
        match k.code {
            KeyCode::Char('m') | KeyCode::Char('M') if buf.is_empty() => {
                return Ok(Some(min_total));
            }
            KeyCode::Char('x') | KeyCode::Char('X') if buf.is_empty() => {
                return Ok(Some(max_total));
            }
            KeyCode::Char('h') | KeyCode::Char('H') if buf.is_empty() => {
                return Ok(Some(half_pot));
            }
            KeyCode::Char('t') | KeyCode::Char('T') if buf.is_empty() => {
                return Ok(Some(two_thirds));
            }
            KeyCode::Char('p') | KeyCode::Char('P') if buf.is_empty() => {
                return Ok(Some(pot_size));
            }
            KeyCode::Char('o') | KeyCode::Char('O') if buf.is_empty() => {
                return Ok(Some(over_pot));
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                buf.push(c);
                err = None;
            }
            KeyCode::Backspace => {
                buf.pop();
                err = None;
            }
            KeyCode::Enter => {
                if let Ok(v) = buf.parse::<u64>() {
                    if v < min_total {
                        err = Some(format!("too low (min ${min_total})"));
                        buf.clear();
                        continue;
                    }
                    if v > max_total {
                        err = Some(format!("too high (max ${max_total})"));
                        buf.clear();
                        continue;
                    }
                    return Ok(Some(v));
                }
                err = Some("invalid number".to_string());
                buf.clear();
            }
            KeyCode::Esc => return Ok(None),
            _ => {}
        }
    }
}

fn redraw_with(overlay: Overlay<'_>) -> io::Result<()> {
    let snap = LAST.with(|c| c.borrow().clone());
    if let Some(snap) = snap {
        draw(&snap, overlay)
    } else {
        Ok(())
    }
}

fn draw(snap: &Snapshot, overlay: Overlay<'_>) -> io::Result<()> {
    TERM.with(|c| -> io::Result<()> {
        let mut borrowed = c.borrow_mut();
        let term = borrowed
            .as_mut()
            .ok_or_else(|| io::Error::other("ratatui terminal not initialised"))?;
        term.draw(|f| draw_frame(f, snap, &overlay))?;
        Ok(())
    })
}

fn draw_frame(f: &mut ratatui::Frame<'_>, snap: &Snapshot, overlay: &Overlay<'_>) {
    let overlay_h = overlay_height(overlay);
    // 摊牌帧需要更多 log 空间，方便看到牌型描述、最佳五张以及 voice。
    let log_h: u16 = if snap.reveal_all { 14 } else { 7 };
    let chunks = RLayout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),                            // header
            Constraint::Length(9),                            // board + hero desc
            Constraint::Min(5),                               // players
            Constraint::Length(log_h),                        // log
            Constraint::Length(if overlay_h > 0 { overlay_h } else { 0 }),
        ])
        .split(f.area());

    draw_header(f, chunks[0], snap);
    draw_board(f, chunks[1], snap);
    draw_players(f, chunks[2], snap);
    draw_log(f, chunks[3], snap);
    if overlay_h > 0 {
        draw_overlay(f, chunks[4], overlay);
    }
}

fn overlay_height(overlay: &Overlay<'_>) -> u16 {
    match overlay {
        Overlay::None => 0,
        Overlay::Prompt(_) | Overlay::Action { .. } | Overlay::ConfirmAllIn(_) => 3,
        Overlay::Raise { .. } => 5,
    }
}

fn draw_header(f: &mut ratatui::Frame<'_>, area: Rect, snap: &Snapshot) {
    let pot = snap.state.total_pot();
    let bb = snap.state.bb;
    let pot_color = if pot >= bb * 50 {
        Color::Yellow
    } else {
        Color::White
    };
    let line = Line::from(vec![
        Span::styled(
            " Texas Hold'em ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            snap.state.stage.label_zh(),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw("   底池 "),
        Span::styled(
            format!("${}", pot),
            Style::default().fg(pot_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("   下注 "),
        Span::styled(
            format!("${}", snap.state.current_bet),
            Style::default().fg(Color::Green),
        ),
        Span::raw("   MinRaise "),
        Span::styled(
            format!("${}", snap.state.min_raise),
            Style::default().fg(Color::Magenta),
        ),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            " ratatui ",
            Style::default().add_modifier(Modifier::BOLD),
        ));
    f.render_widget(Paragraph::new(line).block(block), area);
}

fn draw_board(f: &mut ratatui::Frame<'_>, area: Rect, snap: &Snapshot) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Board ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = RLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(0)])
        .split(inner);

    let cell = 8u16;
    let total = cell * 5;
    let pad = rows[0].width.saturating_sub(total) / 2;
    for i in 0..5 {
        let x = rows[0].x + pad + cell * i as u16;
        let card_area = Rect {
            x,
            y: rows[0].y,
            width: 7,
            height: rows[0].height.min(7),
        };
        if i < snap.state.community.len() {
            render_card(f, card_area, snap.state.community[i], false);
        } else {
            render_card_back(f, card_area);
        }
    }

    if let Some(seat) = snap.hero_seat
        && snap.state.community.len() >= 3
        && snap.state.players[seat].status != PlayerStatus::Folded
        && let Some(hole) = snap.state.players[seat].hole
        && let Some(desc) = describe_hero(hole, &snap.state.community)
    {
        let line = Line::from(vec![
            Span::styled("Your: ", Style::default().fg(Color::Cyan)),
            Span::styled(card_token(hole[0]), card_style(hole[0])),
            Span::raw(" "),
            Span::styled(card_token(hole[1]), card_style(hole[1])),
            Span::styled(format!(" → {desc}"), Style::default().fg(Color::Cyan)),
        ]);
        f.render_widget(Paragraph::new(line), rows[1]);
    }
}

fn draw_players(f: &mut ratatui::Frame<'_>, area: Rect, snap: &Snapshot) {
    let title = if snap.hud.is_some() {
        " Players (HUD) "
    } else {
        " Players "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let n = snap.state.players.len();
    if n == 0 || inner.height < 1 {
        return;
    }
    let mut lines: Vec<Line> = Vec::with_capacity(n + 1);
    let header_text = if snap.hud.is_some() {
        format!(
            "    {:<3} {:<3} {:<10} {:>7}  {:<13} {:<17} Hole",
            "#", "POS", "Name", "Stack", "Status", "HUD"
        )
    } else {
        format!(
            "    {:<3} {:<3} {:<10} {:>7}  {:<13} Hole",
            "#", "POS", "Name", "Stack", "Status"
        )
    };
    lines.push(Line::styled(
        header_text,
        Style::default().add_modifier(Modifier::BOLD),
    ));

    for (i, p) in snap.state.players.iter().enumerate() {
        let is_hero = Some(i) == snap.hero_seat;
        let is_acting = snap.state.to_act == Some(i);
        let is_winner = snap.winners.contains(&i);
        let is_tilt = snap.tilt_marks.get(i).copied().unwrap_or(false);

        let mut style = Style::default();
        if is_winner {
            style = style.fg(Color::Green);
        } else if is_acting {
            style = style.fg(Color::Yellow);
        } else if matches!(p.status, PlayerStatus::Folded | PlayerStatus::SitOut) {
            style = style.fg(Color::DarkGray);
        }
        if is_hero {
            style = style.add_modifier(Modifier::BOLD);
        }

        let marker = if is_acting { ">> " } else { "   " };
        let pos = position_label(&snap.state, i);
        let status_str = match p.status {
            PlayerStatus::Active => {
                if p.committed_round > 0 {
                    format!("bet ${}", p.committed_round)
                } else {
                    "—".to_string()
                }
            }
            PlayerStatus::AllIn => "ALL-IN".to_string(),
            PlayerStatus::Folded => "fold".to_string(),
            PlayerStatus::SitOut => "sit-out".to_string(),
        };
        let prefix = if let Some(hud) = &snap.hud {
            let h = hud.get(i).map(String::as_str).unwrap_or("VP-- PF-- AF--");
            format!(
                "{marker} {:<3} {:<3} {:<10} {:>7}  {:<13} {:<17} ",
                i,
                pos,
                p.name,
                format!("${}", p.stack),
                status_str,
                h,
            )
        } else {
            format!(
                "{marker} {:<3} {:<3} {:<10} {:>7}  {:<13} ",
                i,
                pos,
                p.name,
                format!("${}", p.stack),
                status_str,
            )
        };

        let mut spans = vec![Span::styled(prefix, style)];
        // 手牌
        if let Some(hole) = p.hole {
            if (is_hero || snap.reveal_all) && p.status != PlayerStatus::Folded {
                spans.push(Span::styled(card_token(hole[0]), card_style(hole[0])));
                spans.push(Span::raw(" "));
                spans.push(Span::styled(card_token(hole[1]), card_style(hole[1])));
            } else if p.status == PlayerStatus::Folded {
                if let Some(rc) = p.last_revealed {
                    spans.push(Span::styled(
                        "秀: ",
                        Style::default().fg(Color::DarkGray),
                    ));
                    spans.push(Span::styled(card_token(rc), card_style(rc)));
                }
            } else {
                spans.push(Span::styled(
                    "[??] [??]",
                    Style::default().fg(CARD_BACK),
                ));
            }
        }
        if is_tilt {
            spans.push(Span::styled("  [T]", Style::default().fg(Color::Red)));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_log(f: &mut ratatui::Frame<'_>, area: Rect, snap: &Snapshot) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Log ");
    // 内部高度 = 外部 - 上下边框；按它取尾部，保证最近发生的（含牌型描述）一定可见。
    let visible = block.inner(area).height as usize;
    let take = snap.log.len().saturating_sub(visible.max(1));
    let lines: Vec<Line> = snap.log[take..]
        .iter()
        .map(|s| {
            Line::from(vec![
                Span::styled("· ", Style::default().fg(Color::DarkGray)),
                Span::raw(s.clone()),
            ])
        })
        .collect();
    let p = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn draw_overlay(f: &mut ratatui::Frame<'_>, area: Rect, overlay: &Overlay<'_>) {
    match overlay {
        Overlay::None => {}
        Overlay::Prompt(s) => {
            let p = Paragraph::new(Line::styled(
                format!("  {s}"),
                Style::default().fg(Color::Cyan),
            ))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded),
            );
            f.render_widget(p, area);
        }
        Overlay::Action { can_check, to_call } => {
            let menu = if *can_check {
                "  Your move: [F]old  [K]check  [R]aise  [A]ll-in  (Esc to quit)".to_string()
            } else {
                format!(
                    "  Your move: [F]old  [C]all ${}  [R]aise  [A]ll-in  (Esc to quit)",
                    to_call
                )
            };
            let p = Paragraph::new(Line::styled(menu, Style::default().fg(Color::Cyan)))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(" Action "),
                );
            f.render_widget(p, area);
        }
        Overlay::ConfirmAllIn(total) => {
            let p = Paragraph::new(Line::styled(
                format!(
                    "  [!] Confirm ALL-IN to ${} ? Press [A] again, any other key cancels",
                    total
                ),
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" Confirm "),
            );
            f.render_widget(p, area);
        }
        Overlay::Raise {
            min_total,
            max_total,
            half_pot,
            two_thirds,
            pot_size,
            over_pot,
            buf,
            err,
        } => {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" Raise to ");
            let inner = block.inner(area);
            f.render_widget(block, area);
            let line1 = Line::from(vec![
                Span::raw(format!("  min ${} – max ${}: ", min_total, max_total)),
                Span::styled("[M]min ", Style::default().fg(Color::Yellow)),
                Span::raw(format!("[H]½pot=${} ", half_pot)),
                Span::raw(format!("[T]2/3pot=${} ", two_thirds)),
                Span::raw(format!("[P]pot=${} ", pot_size)),
                Span::raw(format!("[O]2×pot=${} ", over_pot)),
                Span::styled("[X]all-in", Style::default().fg(Color::Yellow)),
            ]);
            let mut line2_spans = vec![
                Span::raw("  digits + Enter (Esc to cancel): "),
                Span::styled(
                    if buf.is_empty() {
                        "_".to_string()
                    } else {
                        (*buf).to_string()
                    },
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ];
            if let Some(e) = err {
                line2_spans.push(Span::styled(
                    format!("   ← {e}"),
                    Style::default().fg(Color::Red),
                ));
            }
            f.render_widget(
                Paragraph::new(vec![line1, Line::from(line2_spans)]),
                inner,
            );
        }
    }
}

/// 5×7 卡牌艺术（与 PoC 一致）。
fn render_card(f: &mut ratatui::Frame<'_>, area: Rect, card: Card, hero: bool) {
    let color = card_color(card.suit());
    let style = Style::default().fg(color);
    let rank = card.rank().label();
    let suit = card.suit().glyph().to_string();
    let line1 = format!("{:<2}   ", rank);
    let line2 = "     ".to_string();
    let line3 = format!("  {}  ", suit);
    let line4 = "     ".to_string();
    let line5 = format!("   {:>2}", rank);
    let mut border = Style::default().fg(color);
    if hero {
        border = border.add_modifier(Modifier::BOLD);
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border);
    let lines = vec![
        Line::styled(line1, style),
        Line::styled(line2, style),
        Line::styled(line3, style),
        Line::styled(line4, style),
        Line::styled(line5, style),
    ];
    f.render_widget(
        Paragraph::new(lines).block(block).alignment(Alignment::Left),
        area,
    );
}

fn render_card_back(f: &mut ratatui::Frame<'_>, area: Rect) {
    let style = Style::default().fg(CARD_BACK);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style);
    let lines = vec![
        Line::styled("░░░░░", style),
        Line::styled("░ ? ░", style),
        Line::styled("░░░░░", style),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Center),
        area,
    );
}

fn card_token(c: Card) -> String {
    format!("[{}{}]", c.rank().label(), c.suit().glyph())
}

fn card_style(c: Card) -> Style {
    Style::default().fg(card_color(c.suit()))
}

fn card_color(s: Suit) -> Color {
    if s.is_red() { Color::Red } else { Color::White }
}

fn position_label(state: &HandState, seat: usize) -> &'static str {
    if seat == state.button {
        return "D";
    }
    let n = state.players.len();
    let active: Vec<usize> = (0..n)
        .map(|k| (state.button + k) % n)
        .filter(|&i| state.players[i].status != PlayerStatus::SitOut)
        .collect();
    if active.len() == 2 {
        if active[1] == seat {
            return "BB";
        }
        return "";
    }
    if active.get(1).copied() == Some(seat) {
        "SB"
    } else if active.get(2).copied() == Some(seat) {
        "BB"
    } else {
        ""
    }
}
