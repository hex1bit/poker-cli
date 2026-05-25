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
use crate::game::state::{HandState, PlayerStatus};

/// 移到下一行行首（raw mode 友好）。
fn nl<W: Write + QueueableCommand>(out: &mut W) -> std::io::Result<()> {
    out.queue(cursor::MoveToNextLine(1))?;
    Ok(())
}

/// 渲染当前牌局。`hero_seat` = 真人座位下标；`reveal_all` 在摊牌时为 true，显示所有人手牌。
/// `winners` 列表中的座位会高亮成绿色；`tilt_marks[seat]` 为 true 时该 bot 行尾显示 [T]。
pub fn render(
    state: &HandState,
    hero_seat: Option<usize>,
    log: &[String],
    reveal_all: bool,
    winners: &[usize],
    tilt_marks: &[bool],
) -> std::io::Result<()> {
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
    write!(
        out,
        "Texas Hold'em — {:?}   Pot ",
        state.stage,
    )?;
    if let Some(c) = pot_color {
        out.queue(SetForegroundColor(c))?;
    }
    write!(out, "${}", pot)?;
    if pot_color.is_some() {
        out.queue(ResetColor)?;
    }
    write!(out, "   Bet ${}   MinRaise ${}", state.current_bet, state.min_raise)?;
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
    nl(&mut out)?;

    // 玩家表（普通对齐列）
    write!(
        out,
        "    {:<3} {:<3} {:<10} {:>7}  {:<13} {}",
        "#", "POS", "Name", "Stack", "Status", "Hole"
    )?;
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
        } else if p.status == PlayerStatus::Folded {
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
    let mut out = stdout();
    out.execute(crossterm::terminal::EnterAlternateScreen)?;
    out.execute(cursor::Hide)?;
    crossterm::terminal::enable_raw_mode()?;
    Ok(())
}

/// 退出备用屏幕缓冲区。
pub fn leave_screen() -> std::io::Result<()> {
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
) -> std::io::Result<()> {
    render(state, None, log, true, winners, tilt_marks)
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
