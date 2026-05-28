//! ratatui PoC：用 widget 渲染一张德州扑克牌桌的演示。
//!
//! 演示要点：
//! - `Terminal::new(CrosstermBackend)` 进入备用屏幕 + raw 模式
//! - Layout 垂直分割：顶部信息条 / 公共牌 / 玩家网格 / 日志
//! - 5×7 ASCII 卡牌艺术（Block + Paragraph + Line/Span）
//! - 红色花色高亮、隐藏牌占位、当前 hero 高亮
//! - 主循环：每 1.2s 自动推进阶段，Space 手动推进，Esc/q 退出
//!
//! 运行：`cargo run --bin ratatui_poc`

use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use poker::card::{Card, Rank, Suit};

/// 演示中的玩家信息。
struct DemoPlayer {
    name: &'static str,
    stack: u64,
    bet: u64,
    folded: bool,
    is_hero: bool,
    hole: [Card; 2],
}

/// 演示牌局阶段。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Preflop,
    Flop,
    Turn,
    River,
    Showdown,
}

impl Stage {
    fn label(self) -> &'static str {
        match self {
            Stage::Preflop => "翻牌前",
            Stage::Flop => "翻牌",
            Stage::Turn => "转牌",
            Stage::River => "河牌",
            Stage::Showdown => "摊牌",
        }
    }

    fn next(self) -> Stage {
        match self {
            Stage::Preflop => Stage::Flop,
            Stage::Flop => Stage::Turn,
            Stage::Turn => Stage::River,
            Stage::River => Stage::Showdown,
            Stage::Showdown => Stage::Preflop,
        }
    }

    /// 该阶段公共牌张数。
    fn board_count(self) -> usize {
        match self {
            Stage::Preflop => 0,
            Stage::Flop => 3,
            Stage::Turn => 4,
            Stage::River | Stage::Showdown => 5,
        }
    }
}

struct AppState {
    stage: Stage,
    pot: u64,
    bet: u64,
    board: [Card; 5],
    players: Vec<DemoPlayer>,
    log: Vec<String>,
    auto: bool,
}

impl AppState {
    fn sample() -> Self {
        let board = [
            Card::new(Rank::Ace, Suit::Spades),
            Card::new(Rank::King, Suit::Hearts),
            Card::new(Rank::Seven, Suit::Diamonds),
            Card::new(Rank::Two, Suit::Clubs),
            Card::new(Rank::Ten, Suit::Hearts),
        ];
        let players = vec![
            DemoPlayer {
                name: "You",
                stack: 1980,
                bet: 60,
                folded: false,
                is_hero: true,
                hole: [
                    Card::new(Rank::Ace, Suit::Diamonds),
                    Card::new(Rank::King, Suit::Clubs),
                ],
            },
            DemoPlayer {
                name: "Alice",
                stack: 1820,
                bet: 60,
                folded: false,
                is_hero: false,
                hole: [
                    Card::new(Rank::Queen, Suit::Spades),
                    Card::new(Rank::Queen, Suit::Hearts),
                ],
            },
            DemoPlayer {
                name: "Bob",
                stack: 0,
                bet: 0,
                folded: true,
                is_hero: false,
                hole: [
                    Card::new(Rank::Five, Suit::Clubs),
                    Card::new(Rank::Three, Suit::Diamonds),
                ],
            },
            DemoPlayer {
                name: "Carol",
                stack: 2400,
                bet: 60,
                folded: false,
                is_hero: false,
                hole: [
                    Card::new(Rank::Jack, Suit::Hearts),
                    Card::new(Rank::Ten, Suit::Spades),
                ],
            },
        ];
        Self {
            stage: Stage::Flop,
            pot: 240,
            bet: 60,
            board,
            players,
            log: vec![
                "Bob folds.".into(),
                "Alice bets $60.".into(),
                "Carol calls $60.".into(),
            ],
            auto: true,
        }
    }

    fn advance(&mut self) {
        let next = self.stage.next();
        self.log.push(format!("→ 进入 {}", next.label()));
        if self.log.len() > 12 {
            let drop = self.log.len() - 12;
            self.log.drain(0..drop);
        }
        self.stage = next;
    }
}

fn main() -> io::Result<()> {
    let mut terminal = setup_terminal()?;
    let res = run(&mut terminal);
    restore_terminal(&mut terminal)?;
    res
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    let mut app = AppState::sample();
    let tick = Duration::from_millis(1200);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| draw(f, &app))?;

        let timeout = tick
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::ZERO);
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(()),
                    KeyCode::Char(' ') | KeyCode::Enter => app.advance(),
                    KeyCode::Char('a') | KeyCode::Char('A') => app.auto = !app.auto,
                    _ => {}
                }
            }
        }
        if last_tick.elapsed() >= tick {
            if app.auto {
                app.advance();
            }
            last_tick = Instant::now();
        }
    }
}

fn draw(f: &mut ratatui::Frame<'_>, app: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // 顶部信息条
            Constraint::Length(9), // 公共牌区域
            Constraint::Min(8),    // 玩家网格
            Constraint::Length(8), // 日志
        ])
        .split(f.area());

    draw_header(f, chunks[0], app);
    draw_board(f, chunks[1], app);
    draw_players(f, chunks[2], app);
    draw_log(f, chunks[3], app);
}

fn draw_header(f: &mut ratatui::Frame<'_>, area: Rect, app: &AppState) {
    let line = Line::from(vec![
        Span::styled(
            " Texas Hold'em ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  阶段 "),
        Span::styled(
            app.stage.label(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   底池 "),
        Span::styled(
            format!("${}", app.pot),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   当前下注 "),
        Span::styled(format!("${}", app.bet), Style::default().fg(Color::Green)),
        Span::raw("    [Space] 推进  [A] auto  [Esc] 退出"),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            " ratatui PoC ",
            Style::default().add_modifier(Modifier::BOLD),
        ));
    let p = Paragraph::new(line).block(block);
    f.render_widget(p, area);
}

fn draw_board(f: &mut ratatui::Frame<'_>, area: Rect, app: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Board ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let visible = app.stage.board_count();
    let cell = 8u16; // 7 width + 1 gap
    let total = cell * 5;
    let pad = inner.width.saturating_sub(total) / 2;

    for i in 0..5 {
        let x = inner.x + pad + cell * i as u16;
        let card_area = Rect {
            x,
            y: inner.y,
            width: 7,
            height: inner.height.min(7),
        };
        if i < visible {
            render_card(f, card_area, app.board[i], false);
        } else {
            render_card_back(f, card_area);
        }
    }
}

fn draw_players(f: &mut ratatui::Frame<'_>, area: Rect, app: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Players ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let n = app.players.len() as u16;
    if n == 0 {
        return;
    }
    let constraints: Vec<Constraint> = (0..n).map(|_| Constraint::Ratio(1, n as u32)).collect();
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(inner);

    let reveal = app.stage == Stage::Showdown;
    for (i, p) in app.players.iter().enumerate() {
        draw_player_cell(f, cols[i], p, reveal);
    }
}

fn draw_player_cell(f: &mut ratatui::Frame<'_>, area: Rect, p: &DemoPlayer, reveal: bool) {
    let title_style = if p.is_hero {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else if p.folded {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };
    let title = format!(" {} ", p.name);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .title(Span::styled(title, title_style));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(5)])
        .split(inner);

    let status_line = if p.folded {
        Line::from(Span::styled(
            "fold",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        Line::from(vec![
            Span::raw("Stack "),
            Span::styled(
                format!("${}", p.stack),
                Style::default().fg(Color::Green),
            ),
            Span::raw("  Bet "),
            Span::styled(
                format!("${}", p.bet),
                Style::default().fg(Color::Yellow),
            ),
        ])
    };
    f.render_widget(Paragraph::new(status_line), rows[0]);

    // 两张手牌并排
    let card_row = rows[1];
    let card_w = 7u16;
    let gap = 1u16;
    let total = card_w * 2 + gap;
    let pad = card_row.width.saturating_sub(total) / 2;
    let show = (p.is_hero || reveal) && !p.folded;
    for k in 0..2 {
        let x = card_row.x + pad + (card_w + gap) * k as u16;
        let area = Rect {
            x,
            y: card_row.y,
            width: card_w,
            height: card_row.height.min(5),
        };
        if show {
            render_card(f, area, p.hole[k], p.is_hero);
        } else if p.folded {
            // 弃牌不显示
            f.render_widget(Paragraph::new(""), area);
        } else {
            render_card_back(f, area);
        }
    }
}

fn draw_log(f: &mut ratatui::Frame<'_>, area: Rect, app: &AppState) {
    let lines: Vec<Line> = app
        .log
        .iter()
        .map(|s| {
            Line::from(vec![
                Span::styled("· ", Style::default().fg(Color::DarkGray)),
                Span::raw(s.clone()),
            ])
        })
        .collect();
    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" Log "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

/// 渲染一张正面牌：5 行 × 7 列 ASCII 艺术。
fn render_card(f: &mut ratatui::Frame<'_>, area: Rect, card: Card, hero: bool) {
    let color = if card.suit().is_red() {
        Color::Red
    } else {
        Color::White
    };
    let style = Style::default().fg(color).bg(Color::Black);
    let rank = card.rank().label();
    let suit = card.suit().glyph().to_string();
    // 7 字符宽：把 rank 左/右对齐，suit 居中。
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
    let p = Paragraph::new(lines).block(block).alignment(Alignment::Left);
    f.render_widget(p, area);
}

/// 渲染一张背面牌（隐藏）。
fn render_card_back(f: &mut ratatui::Frame<'_>, area: Rect) {
    let style = Style::default().fg(Color::Blue);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style);
    let lines = vec![
        Line::styled("░░░░░", style),
        Line::styled("░ ? ░", style),
        Line::styled("░░░░░", style),
    ];
    let p = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Center);
    f.render_widget(p, area);
}
