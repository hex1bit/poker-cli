//! 终端渲染：清屏后重绘当前牌局，使用普通文本表格（无圆桌/无装饰边框）。
//!
//! 关键：终端处于 raw mode，`\n` 不会回到行首，所有换行必须通过
//! `cursor::MoveToNextLine` 完成。

use std::io::{Write, stdout};

use crossterm::{
    cursor,
    style::{Color, ResetColor, SetForegroundColor},
    terminal::{Clear, ClearType},
    {ExecutableCommand, QueueableCommand},
};

use crate::card::Card;
use crate::config::Layout;
use crate::game::state::{HandState, PlayerStatus};
use crate::hand_eval::describe_hero;

const SQUARE_CELL_WIDTH: usize = 17;
const SQUARE_CENTER_WIDTH: usize = 32;

#[derive(Clone, Copy)]
pub struct RenderOptions<'a> {
    pub hero_seat: Option<usize>,
    pub log: &'a [String],
    pub reveal_all: bool,
    pub winners: &'a [usize],
    pub tilt_marks: &'a [bool],
    pub hud: Option<&'a [String]>,
    pub layout: Layout,
}

/// 移到下一行行首（raw mode 友好）。
fn nl<W: Write + QueueableCommand>(out: &mut W) -> std::io::Result<()> {
    out.queue(cursor::MoveToNextLine(1))?;
    Ok(())
}

/// 渲染当前牌局。`hero_seat` = 真人座位下标；`reveal_all` 在摊牌时为 true，显示所有人手牌。
/// `winners` 列表中的座位会高亮成绿色；`tilt_marks[seat]` 为 true 时该 bot 行尾显示 [T]。
pub fn render(state: &HandState, opts: RenderOptions<'_>) -> std::io::Result<()> {
    if opts.layout == Layout::Ratatui {
        return crate::ui::ratatui_render::render(state, opts);
    }
    if opts.layout == Layout::Square {
        return render_square(state, opts);
    }
    let hero_seat = opts.hero_seat;
    let log = opts.log;
    let reveal_all = opts.reveal_all;
    let winners = opts.winners;
    let tilt_marks = opts.tilt_marks;
    let hud = opts.hud;
    let mut out = stdout();
    out.queue(Clear(ClearType::All))?;
    out.queue(cursor::MoveTo(0, 0))?;

    // 顶部：阶段 / 底池 / 公共牌
    let pot = state.total_pot();
    // 底池 ≥ 2*bb*10 时高亮金色（粗略阈值，不依赖 initial stack）
    let pot_color = if pot >= state.bb * 50 {
        Some(Color::Yellow)
    } else {
        None
    };
    write!(out, "Texas Hold'em — {}   Pot ", state.stage.label_zh(),)?;
    if let Some(c) = pot_color {
        out.queue(SetForegroundColor(c))?;
    }
    write!(out, "${}", pot)?;
    if pot_color.is_some() {
        out.queue(ResetColor)?;
    }
    write!(
        out,
        "   Bet ${}   MinRaise ${}",
        state.current_bet, state.min_raise
    )?;
    nl(&mut out)?;
    write!(out, "Board: ")?;
    if state.community.is_empty() {
        write!(out, "—")?;
    } else {
        for c in &state.community {
            write_card(&mut out, *c)?;
            write!(out, " ")?;
        }
    }
    nl(&mut out)?;
    if let Some(seat) = hero_seat
        && state.community.len() >= 3
        && state.players[seat].status != PlayerStatus::Folded
        && let Some(hole) = state.players[seat].hole
        && let Some(desc) = describe_hero(hole, &state.community)
    {
        out.queue(SetForegroundColor(Color::Cyan))?;
        write!(out, "Your: ")?;
        write_card(&mut out, hole[0])?;
        out.queue(SetForegroundColor(Color::Cyan))?;
        write!(out, " ")?;
        write_card(&mut out, hole[1])?;
        out.queue(SetForegroundColor(Color::Cyan))?;
        write!(out, " → {desc}")?;
        out.queue(ResetColor)?;
        nl(&mut out)?;
    }
    nl(&mut out)?;

    // 玩家表（普通对齐列）
    if hud.is_some() {
        write!(
            out,
            "    {:<3} {:<3} {:<10} {:>7}  {:<13} {:<17} Hole",
            "#", "POS", "Name", "Stack", "Status", "HUD"
        )?;
    } else {
        write!(
            out,
            "    {:<3} {:<3} {:<10} {:>7}  {:<13} Hole",
            "#", "POS", "Name", "Stack", "Status"
        )?;
    }
    nl(&mut out)?;
    for (i, p) in state.players.iter().enumerate() {
        let is_hero = Some(i) == hero_seat;
        let is_acting = state.to_act == Some(i);
        let is_winner = winners.contains(&i);
        let is_tilted = tilt_marks.get(i).copied().unwrap_or(false);
        let pos_label = position_label(state, i);
        let marker = if is_acting { ">> " } else { "   " };

        if is_winner {
            out.queue(SetForegroundColor(Color::Green))?;
        } else if is_acting {
            out.queue(SetForegroundColor(Color::Yellow))?;
        } else if matches!(p.status, PlayerStatus::Folded | PlayerStatus::SitOut) {
            out.queue(SetForegroundColor(Color::DarkGrey))?;
        }

        write!(
            out,
            "{marker} {:<3} {:<3} {:<10} {:>7}  ",
            i,
            pos_label,
            p.name,
            format!("${}", p.stack),
        )?;

        let status = match p.status {
            PlayerStatus::Active => {
                if p.committed_round > 0 {
                    format!("bet ${}", p.committed_round)
                } else {
                    String::from("—")
                }
            }
            PlayerStatus::AllIn => "ALL-IN".to_string(),
            PlayerStatus::Folded => "fold".to_string(),
            PlayerStatus::SitOut => "sit-out".to_string(),
        };
        write!(out, "{:<13} ", status)?;
        if let Some(hud) = hud {
            let text = hud.get(i).map(String::as_str).unwrap_or("VP-- PF-- AF--");
            write!(out, "{:<17} ", text)?;
        }

        // 手牌
        if let Some(hole) = p.hole {
            if (is_hero || reveal_all) && p.status != PlayerStatus::Folded {
                write_card(&mut out, hole[0])?;
                write!(out, " ")?;
                write_card(&mut out, hole[1])?;
            } else if p.status == PlayerStatus::Folded {
                // 默认不显示，但若 last_revealed 存在则显示一张（show one）
                if let Some(rc) = p.last_revealed {
                    out.queue(SetForegroundColor(Color::DarkGrey))?;
                    write!(out, "秀: ")?;
                    write_card(&mut out, rc)?;
                }
            } else {
                out.queue(SetForegroundColor(Color::Blue))?;
                write!(out, "[??] [??]")?;
            }
        }
        if is_tilted {
            out.queue(SetForegroundColor(Color::Red))?;
            write!(out, "  [T]")?;
        }
        out.queue(ResetColor)?;
        nl(&mut out)?;
    }

    nl(&mut out)?;
    let take = log.len().saturating_sub(8);
    for line in &log[take..] {
        out.queue(SetForegroundColor(Color::DarkGrey))?;
        write!(out, "  · ")?;
        out.queue(ResetColor)?;
        write!(out, "{line}")?;
        nl(&mut out)?;
    }
    nl(&mut out)?;

    out.flush()?;
    Ok(())
}

#[derive(Clone, Copy)]
struct SquareCtx<'a> {
    hero_seat: Option<usize>,
    reveal_all: bool,
    winners: &'a [usize],
    tilt_marks: &'a [bool],
    hud: Option<&'a [String]>,
}

struct SeatCell {
    seat: Option<usize>,
    lines: [String; 3],
}

fn render_square(state: &HandState, opts: RenderOptions<'_>) -> std::io::Result<()> {
    let mut out = stdout();
    out.queue(Clear(ClearType::All))?;
    out.queue(cursor::MoveTo(0, 0))?;

    write!(
        out,
        "Texas Hold'em — {}   Pot ${}   Bet ${}",
        state.stage.label_zh(),
        state.total_pot(),
        state.current_bet
    )?;
    nl(&mut out)?;
    write!(out, "Board: ")?;
    if state.community.is_empty() {
        write!(out, "—")?;
    } else {
        for c in &state.community {
            write_card(&mut out, *c)?;
            write!(out, " ")?;
        }
    }
    nl(&mut out)?;
    if let Some(seat) = opts.hero_seat
        && state.community.len() >= 3
        && state.players[seat].status != PlayerStatus::Folded
        && let Some(hole) = state.players[seat].hole
        && let Some(desc) = describe_hero(hole, &state.community)
    {
        out.queue(SetForegroundColor(Color::Cyan))?;
        write!(out, "Your: ")?;
        write_card(&mut out, hole[0])?;
        out.queue(SetForegroundColor(Color::Cyan))?;
        write!(out, " ")?;
        write_card(&mut out, hole[1])?;
        out.queue(SetForegroundColor(Color::Cyan))?;
        write!(out, " → {desc}")?;
        out.queue(ResetColor)?;
        nl(&mut out)?;
    }
    nl(&mut out)?;

    let bots: Vec<usize> = (1..state.players.len()).collect();
    let top: Vec<Option<usize>> = fixed_slots(bots.iter().copied().take(4), 4);
    let side: Vec<usize> = bots.iter().copied().skip(4).take(4).collect();
    let rest: Vec<usize> = bots.iter().copied().skip(8).collect();
    let bottom = [None, Some(0), rest.first().copied(), rest.get(1).copied()];

    let ctx = SquareCtx {
        hero_seat: opts.hero_seat,
        reveal_all: opts.reveal_all,
        winners: opts.winners,
        tilt_marks: opts.tilt_marks,
        hud: opts.hud,
    };

    write_seat_strip(&mut out, state, ctx, &top)?;
    write_center_border(&mut out)?;
    nl(&mut out)?;
    for row in 0..2 {
        let left = side.get(row).copied();
        let right = side.get(row + 2).copied();
        write_side_row(&mut out, state, ctx, left, right, row)?;
    }
    write_center_border(&mut out)?;
    nl(&mut out)?;
    write_seat_strip(&mut out, state, ctx, &bottom)?;

    nl(&mut out)?;
    let take = opts.log.len().saturating_sub(8);
    for line in &opts.log[take..] {
        out.queue(SetForegroundColor(Color::DarkGrey))?;
        write!(out, "  · ")?;
        out.queue(ResetColor)?;
        write!(out, "{line}")?;
        nl(&mut out)?;
    }
    out.flush()?;
    Ok(())
}

fn write_center_border<W: Write + QueueableCommand>(out: &mut W) -> std::io::Result<()> {
    write!(
        out,
        "  {}  +{:-<width$}+  {}",
        " ".repeat(SQUARE_CELL_WIDTH),
        "",
        " ".repeat(SQUARE_CELL_WIDTH),
        width = SQUARE_CENTER_WIDTH
    )
}

fn write_seat_strip<W: Write + QueueableCommand>(
    out: &mut W,
    state: &HandState,
    ctx: SquareCtx<'_>,
    seats: &[Option<usize>],
) -> std::io::Result<()> {
    let cards: Vec<SeatCell> = seats
        .iter()
        .map(|&seat| SeatCell {
            seat,
            lines: seat
                .map(|seat| seat_card_lines(state, seat, ctx))
                .unwrap_or_else(empty_seat_card),
        })
        .collect();
    for line in 0..3 {
        write!(out, "  ")?;
        for card in &cards {
            queue_seat_color(out, state, ctx, card.seat)?;
            write!(out, "{}", card.lines[line])?;
            out.queue(ResetColor)?;
            write!(out, "  ")?;
        }
        nl(out)?;
    }
    Ok(())
}

fn write_side_row<W: Write + QueueableCommand>(
    out: &mut W,
    state: &HandState,
    ctx: SquareCtx<'_>,
    left: Option<usize>,
    right: Option<usize>,
    row: usize,
) -> std::io::Result<()> {
    let left_cell = SeatCell {
        seat: left,
        lines: left
            .map(|seat| seat_card_lines(state, seat, ctx))
            .unwrap_or_else(empty_seat_card),
    };
    let right_cell = SeatCell {
        seat: right,
        lines: right
            .map(|seat| seat_card_lines(state, seat, ctx))
            .unwrap_or_else(empty_seat_card),
    };
    let center = center_lines(state, row);
    for (line, center_line) in center.iter().enumerate() {
        write!(out, "  ")?;
        queue_seat_color(out, state, ctx, left_cell.seat)?;
        write!(out, "{}", left_cell.lines[line])?;
        out.queue(ResetColor)?;
        write!(out, "  |{center_line}|  ")?;
        queue_seat_color(out, state, ctx, right_cell.seat)?;
        write!(out, "{}", right_cell.lines[line])?;
        out.queue(ResetColor)?;
        nl(out)?;
    }
    Ok(())
}

fn queue_seat_color<W: Write + QueueableCommand>(
    out: &mut W,
    state: &HandState,
    ctx: SquareCtx<'_>,
    seat: Option<usize>,
) -> std::io::Result<()> {
    if let Some(seat) = seat {
        let p = &state.players[seat];
        if ctx.winners.contains(&seat) {
            out.queue(SetForegroundColor(Color::Green))?;
        } else if state.to_act == Some(seat) {
            out.queue(SetForegroundColor(Color::Yellow))?;
        } else if matches!(p.status, PlayerStatus::Folded | PlayerStatus::SitOut) {
            out.queue(SetForegroundColor(Color::DarkGrey))?;
        }
    }
    Ok(())
}

fn fixed_slots<I: Iterator<Item = usize>>(seats: I, cells: usize) -> Vec<Option<usize>> {
    let mut slots: Vec<Option<usize>> = seats.map(Some).collect();
    slots.resize(cells, None);
    slots
}

fn seat_card_lines(state: &HandState, seat: usize, ctx: SquareCtx<'_>) -> [String; 3] {
    let p = &state.players[seat];
    let marker = if state.to_act == Some(seat) { ">" } else { " " };
    let win = if ctx.winners.contains(&seat) { "*" } else { "" };
    let tilt = if ctx.tilt_marks.get(seat).copied().unwrap_or(false) {
        "T"
    } else {
        ""
    };
    let hole = if let Some(h) = p.hole {
        if (ctx.hero_seat == Some(seat) || ctx.reveal_all) && p.status != PlayerStatus::Folded {
            format!("[{}] [{}]", h[0], h[1])
        } else if p.status == PlayerStatus::Folded {
            p.last_revealed
                .map(|c| format!("秀:[{c}]"))
                .unwrap_or_else(|| "fold".to_string())
        } else {
            "[??] [??]".to_string()
        }
    } else {
        String::new()
    };
    let pos = position_label(state, seat);
    let status = compact_status(p);
    let hud = ctx.hud.and_then(|h| h.get(seat)).map(|s| compact_hud(s));
    [
        fit_display(
            &format!("{marker}{win}#{seat}{pos} {}{}", p.name, tilt),
            SQUARE_CELL_WIDTH,
        ),
        fit_display(&format!("${} {status}", p.stack), SQUARE_CELL_WIDTH),
        fit_display(
            &format!("{hole} {}", hud.as_deref().unwrap_or("")),
            SQUARE_CELL_WIDTH,
        ),
    ]
}

fn center_lines(state: &HandState, row: usize) -> [String; 3] {
    if row == 0 {
        [
            fit_display(&format!("Pot ${}", state.total_pot()), SQUARE_CENTER_WIDTH),
            fit_display(&board_text(state), SQUARE_CENTER_WIDTH),
            fit_display(&format!("Bet ${}", state.current_bet), SQUARE_CENTER_WIDTH),
        ]
    } else {
        [
            fit_display(state.stage.label_zh(), SQUARE_CENTER_WIDTH),
            fit_display(
                &format!("MinRaise ${}", state.min_raise),
                SQUARE_CENTER_WIDTH,
            ),
            fit_display("", SQUARE_CENTER_WIDTH),
        ]
    }
}

fn board_text(state: &HandState) -> String {
    if state.community.is_empty() {
        "Board: -".to_string()
    } else {
        format!(
            "Board: {}",
            state
                .community
                .iter()
                .map(|c| format!("[{c}]"))
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

fn compact_status(p: &crate::game::state::Player) -> String {
    match p.status {
        PlayerStatus::Active => {
            if p.committed_round > 0 {
                format!("bet{}", p.committed_round)
            } else {
                "-".to_string()
            }
        }
        PlayerStatus::AllIn => "ALLIN".to_string(),
        PlayerStatus::Folded => "fold".to_string(),
        PlayerStatus::SitOut => "out".to_string(),
    }
}

fn compact_hud(s: &str) -> String {
    s.replace("VP", "V").replace("PF", "P").replace("AF", "A")
}

fn fit_display(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    let mut truncated = false;
    for ch in text.chars() {
        let w = char_display_width(ch);
        if used + w > width {
            truncated = true;
            break;
        }
        out.push(ch);
        used += w;
    }
    if truncated && width >= 2 {
        while used + 2 > width {
            if let Some(ch) = out.pop() {
                used = used.saturating_sub(char_display_width(ch));
            } else {
                break;
            }
        }
        out.push_str("..");
        used += 2;
    }
    out.push_str(&" ".repeat(width.saturating_sub(used)));
    out
}

fn char_display_width(ch: char) -> usize {
    if ch.is_ascii() { 1 } else { 2 }
}

fn empty_seat_card() -> [String; 3] {
    [
        " ".repeat(SQUARE_CELL_WIDTH),
        " ".repeat(SQUARE_CELL_WIDTH),
        " ".repeat(SQUARE_CELL_WIDTH),
    ]
}

/// 返回该座位的位置标签：D / SB / BB / 空。
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

/// 在终端上打印一张带颜色的牌。
pub fn write_card<W: Write + QueueableCommand>(out: &mut W, c: Card) -> std::io::Result<()> {
    let color = if c.suit().is_red() {
        Color::Red
    } else {
        Color::White
    };
    out.queue(SetForegroundColor(color))?;
    write!(out, "[{}{}]", c.rank(), c.suit())?;
    out.queue(ResetColor)?;
    Ok(())
}

/// 进入备用屏幕缓冲区，隐藏光标。
pub fn enter_screen() -> std::io::Result<()> {
    enter_screen_for(Layout::Table)
}

/// 与 `enter_screen` 类似，但根据 layout 选择 ratatui 或纯 crossterm 路径。
pub fn enter_screen_for(layout: Layout) -> std::io::Result<()> {
    if layout == Layout::Ratatui {
        return crate::ui::ratatui_render::enter_screen();
    }
    let mut out = stdout();
    out.execute(crossterm::terminal::EnterAlternateScreen)?;
    out.execute(cursor::Hide)?;
    crossterm::terminal::enable_raw_mode()?;
    Ok(())
}

/// 退出备用屏幕缓冲区。
pub fn leave_screen() -> std::io::Result<()> {
    leave_screen_for(Layout::Table)
}

pub fn leave_screen_for(layout: Layout) -> std::io::Result<()> {
    if layout == Layout::Ratatui {
        return crate::ui::ratatui_render::leave_screen();
    }
    let mut out = stdout();
    crossterm::terminal::disable_raw_mode()?;
    out.execute(cursor::Show)?;
    out.execute(crossterm::terminal::LeaveAlternateScreen)?;
    Ok(())
}

/// 摊牌渲染（公开所有人手牌）。
pub fn render_showdown(
    state: &HandState,
    log: &[String],
    winners: &[usize],
    tilt_marks: &[bool],
    hud: Option<&[String]>,
    layout: Layout,
) -> std::io::Result<()> {
    if layout == Layout::Ratatui {
        return crate::ui::ratatui_render::render_showdown(state, log, winners, tilt_marks, hud);
    }
    render(
        state,
        RenderOptions {
            hero_seat: None,
            log,
            reveal_all: true,
            winners,
            tilt_marks,
            hud,
            layout,
        },
    )
}

/// `wait_for_continue` 的用户选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinueAction {
    /// 进入下一手。
    Next,
    /// 回放上一手。
    Replay,
}

/// 阻塞等待按下 Space / Enter / R。
pub fn wait_for_continue(prompt: &str) -> std::io::Result<ContinueAction> {
    wait_for_continue_for(prompt, Layout::Table)
}

pub fn wait_for_continue_for(prompt: &str, layout: Layout) -> std::io::Result<ContinueAction> {
    if layout == Layout::Ratatui {
        return crate::ui::ratatui_render::wait_for_continue(prompt);
    }
    use crossterm::event::{Event, KeyCode, read};
    let mut out = stdout();
    write!(out, "  {prompt} ")?;
    out.flush()?;
    loop {
        if let Event::Key(k) = read()? {
            match k.code {
                KeyCode::Char(' ') | KeyCode::Enter => return Ok(ContinueAction::Next),
                KeyCode::Char('r') | KeyCode::Char('R') => return Ok(ContinueAction::Replay),
                KeyCode::Char('q') | KeyCode::Esc => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "user quit",
                    ));
                }
                _ => {}
            }
        }
    }
}
