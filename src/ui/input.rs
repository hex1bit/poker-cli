//! 真人键盘输入：F/C/R/A/K/Esc。
//!
//! 在 raw 模式下读取按键，必要时弹出 inline 数字输入获取 raise 金额。

use std::io::{Write, stdout};

use crossterm::{
    cursor,
    event::{Event, KeyCode, read},
    style::{Color, ResetColor, SetForegroundColor},
    QueueableCommand,
};

use crate::game::action::Action;
use crate::game::state::HandState;

/// 提示真人玩家行动。
/// 返回 `Err(io::ErrorKind::Interrupted)` 表示用户希望退出游戏。
pub fn prompt_action(state: &HandState, seat: usize) -> std::io::Result<Action> {
    let to_call = state.to_call_for(seat);
    let stack = state.players[seat].stack;
    let can_check = to_call == 0;

    let mut out = stdout();
    out.queue(SetForegroundColor(Color::Cyan))?;
    if can_check {
        write!(
            out,
            "  Your move: [F]old  [K]check  [R]aise  [A]ll-in  (Esc to quit) > "
        )?;
    } else {
        write!(
            out,
            "  Your move: [F]old  [C]all ${}  [R]aise  [A]ll-in  (Esc to quit) > ",
            to_call.min(stack)
        )?;
    }
    out.queue(ResetColor)?;
    out.flush()?;

    loop {
        if let Event::Key(k) = read()? {
            match k.code {
                KeyCode::Char('f') | KeyCode::Char('F') => return Ok(Action::Fold),
                KeyCode::Char('k') | KeyCode::Char('K') if can_check => return Ok(Action::Check),
                KeyCode::Char('c') | KeyCode::Char('C') if !can_check => return Ok(Action::Call),
                KeyCode::Char('a') | KeyCode::Char('A') => return Ok(Action::AllIn),
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    let amt = prompt_raise_amount(state, seat)?;
                    if let Some(total) = amt {
                        if state.current_bet == 0 {
                            return Ok(Action::Bet(total));
                        } else {
                            return Ok(Action::Raise(total));
                        }
                    }
                    // 取消 raise 弹窗回到选项
                    out.queue(SetForegroundColor(Color::Cyan))?;
                    write!(out, "  (cancel) > ")?;
                    out.queue(ResetColor)?;
                    out.flush()?;
                }
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
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

/// 弹窗读取 raise 总额；返回 None 表示用户取消。
fn prompt_raise_amount(state: &HandState, seat: usize) -> std::io::Result<Option<u64>> {
    let min_total = if state.current_bet == 0 {
        state.bb
    } else {
        state.current_bet + state.min_raise
    };
    let max_total = state.players[seat].committed_round + state.players[seat].stack;

    let mut out = stdout();
    out.queue(cursor::MoveToNextLine(1))?;
    out.queue(SetForegroundColor(Color::Yellow))?;
    write!(
        out,
        "  Raise to amount (min ${} – max ${}, Enter to confirm, Esc to cancel): ",
        min_total, max_total
    )?;
    out.queue(ResetColor)?;
    out.flush()?;

    let mut buf = String::new();
    loop {
        if let Event::Key(k) = read()? {
            match k.code {
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    buf.push(c);
                    write!(out, "{c}")?;
                    out.flush()?;
                }
                KeyCode::Backspace => {
                    if buf.pop().is_some() {
                        write!(out, "\u{8} \u{8}")?;
                        out.flush()?;
                    }
                }
                KeyCode::Enter => {
                    if let Ok(v) = buf.parse::<u64>() {
                        if v >= min_total && v <= max_total {
                            return Ok(Some(v));
                        }
                    }
                    // 非法：闪烁提示后继续
                    out.queue(SetForegroundColor(Color::Red))?;
                    write!(out, " ← invalid, retry: ")?;
                    out.queue(ResetColor)?;
                    out.flush()?;
                    buf.clear();
                }
                KeyCode::Esc => return Ok(None),
                _ => {}
            }
        }
    }
}
