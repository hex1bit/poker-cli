//! 真人键盘输入：F/C/R/A/K/Esc。
//!
//! 在 raw 模式下读取按键，必要时弹出 inline 数字输入获取 raise 金额。

use std::io::{Write, stdout};

use crossterm::{
    QueueableCommand, cursor,
    event::{Event, KeyCode, read},
    style::{Color, ResetColor, SetForegroundColor},
};

use crate::config::Layout;
use crate::game::action::Action;
use crate::game::state::HandState;

/// 提示真人玩家行动。
/// 返回 `Err(io::ErrorKind::Interrupted)` 表示用户希望退出游戏。
pub fn prompt_action(state: &HandState, seat: usize) -> std::io::Result<Action> {
    prompt_action_for(state, seat, Layout::Table)
}

/// 与 `prompt_action` 类似，但当 layout=Ratatui 时走 ratatui 路径，避免裸 print
/// 与 ratatui diff 渲染冲突。
pub fn prompt_action_for(
    state: &HandState,
    seat: usize,
    layout: Layout,
) -> std::io::Result<Action> {
    if layout == Layout::Ratatui {
        return crate::ui::ratatui_render::prompt_action(state, seat);
    }
    let to_call = state.to_call_for(seat);
    let stack = state.players[seat].stack;
    let can_check = to_call == 0;

    let mut out = stdout();
    write_action_menu(&mut out, can_check, to_call.min(stack))?;

    loop {
        if let Event::Key(k) = read()? {
            match k.code {
                KeyCode::Char('f') | KeyCode::Char('F') => return Ok(Action::Fold),
                KeyCode::Char('k') | KeyCode::Char('K') if can_check => return Ok(Action::Check),
                KeyCode::Char('c') | KeyCode::Char('C') if !can_check => return Ok(Action::Call),
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    if confirm_allin(state, seat)? {
                        return Ok(Action::AllIn);
                    }
                    write_action_menu(&mut out, can_check, to_call.min(stack))?;
                }
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
                    write_action_menu(&mut out, can_check, to_call.min(stack))?;
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

fn write_action_menu<W: Write + QueueableCommand>(
    out: &mut W,
    can_check: bool,
    to_call_chips: u64,
) -> std::io::Result<()> {
    out.queue(cursor::MoveToNextLine(1))?;
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
            to_call_chips
        )?;
    }
    out.queue(ResetColor)?;
    out.flush()?;
    Ok(())
}

/// 全押二次确认：再按一次 A 提交，其他键取消。
fn confirm_allin(state: &HandState, seat: usize) -> std::io::Result<bool> {
    let total = state.players[seat].committed_round + state.players[seat].stack;
    let mut out = stdout();
    out.queue(cursor::MoveToNextLine(1))?;
    out.queue(SetForegroundColor(Color::Red))?;
    write!(
        out,
        "  [!] Confirm ALL-IN to ${} ? Press [A] again, any other key cancels > ",
        total
    )?;
    out.queue(ResetColor)?;
    out.flush()?;
    loop {
        if let Event::Key(k) = read()? {
            return Ok(matches!(k.code, KeyCode::Char('a') | KeyCode::Char('A')));
        }
    }
}

/// 弹窗读取 raise 总额；返回 None 表示用户取消。
///
/// 支持空 buffer 时一键尺寸：
///   M = min raise / X = max (all-in)
///   H = ½ pot / T = ⅔ pot / P = pot / O = 2× pot
fn prompt_raise_amount(state: &HandState, seat: usize) -> std::io::Result<Option<u64>> {
    let min_total = if state.current_bet == 0 {
        state.bb
    } else {
        state.current_bet + state.min_raise
    };
    let cur_round = state.players[seat].committed_round;
    let max_total = cur_round + state.players[seat].stack;
    // 估算"call 之后"的底池，用于按比例计算 raise 尺寸。
    let to_call = state.current_bet.saturating_sub(cur_round);
    let pot_after_call = state.total_pot() + to_call;
    let cur_bet = state.current_bet;
    let bb = state.bb;
    let preset = |target: u64| target.clamp(min_total, max_total);
    let half_pot = preset(cur_bet.max(bb) + pot_after_call / 2);
    let two_thirds = preset(cur_bet.max(bb) + pot_after_call * 2 / 3);
    let pot_size = preset(cur_bet.max(bb) + pot_after_call);
    let over_pot = preset(cur_bet.max(bb) + pot_after_call * 2);

    let mut out = stdout();
    out.queue(cursor::MoveToNextLine(1))?;
    out.queue(SetForegroundColor(Color::Yellow))?;
    write!(
        out,
        "  Raise to (min ${} – max ${}): [M]min [H]½pot=${} [T]2/3pot=${} [P]pot=${} [O]2×pot=${} [X]all-in",
        min_total, max_total, half_pot, two_thirds, pot_size, over_pot
    )?;
    out.queue(ResetColor)?;
    out.queue(cursor::MoveToNextLine(1))?;
    out.queue(SetForegroundColor(Color::Yellow))?;
    write!(
        out,
        "    or type digits + Enter (Esc to cancel): "
    )?;
    out.queue(ResetColor)?;
    out.flush()?;

    let mut buf = String::new();
    loop {
        if let Event::Key(k) = read()? {
            match k.code {
                // 空 buffer 时的快捷键
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
                        if v < min_total {
                            invalid_hint(&mut out, &format!("too low (min ${min_total})"))?;
                            buf.clear();
                            continue;
                        }
                        if v > max_total {
                            invalid_hint(&mut out, &format!("too high (max ${max_total})"))?;
                            buf.clear();
                            continue;
                        }
                        return Ok(Some(v));
                    }
                    invalid_hint(&mut out, "invalid number")?;
                    buf.clear();
                }
                KeyCode::Esc => return Ok(None),
                _ => {}
            }
        }
    }
}

fn invalid_hint<W: Write + QueueableCommand>(out: &mut W, msg: &str) -> std::io::Result<()> {
    out.queue(SetForegroundColor(Color::Red))?;
    write!(out, " ← {msg}, retry: ")?;
    out.queue(ResetColor)?;
    out.flush()?;
    Ok(())
}
