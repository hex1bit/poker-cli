//! 公共牌发牌动画 + 节奏控制。
//!
//! 在 advance_street 完成后调用，逐张揭示新发的公共牌；
//! 通过 clone HandState 短暂"撤回"已发的牌实现伪动画，最终保持原 state 不变。

use std::thread;
use std::time::Duration;

use crate::config::Layout;
use crate::game::state::HandState;
use crate::ui::render::{RenderOptions, render};

#[derive(Clone, Copy)]
pub struct DealAnimationOptions<'a> {
    pub hero: Option<usize>,
    pub log: &'a [String],
    pub tilt_marks: &'a [bool],
    pub hud: Option<&'a [String]>,
    pub layout: Layout,
    pub new_card_count: usize,
    pub delay_ms: u64,
}

/// 逐张揭示最新发出的 `new_card_count` 张公共牌。
///
/// `delay_ms == 0` 时退化为立即整版重绘一次。
pub fn animate_deal_community(
    state: &HandState,
    opts: DealAnimationOptions<'_>,
) -> std::io::Result<()> {
    if opts.delay_ms == 0 || opts.new_card_count == 0 {
        return render(
            state,
            RenderOptions {
                hero_seat: opts.hero,
                log: opts.log,
                reveal_all: false,
                winners: &[],
                tilt_marks: opts.tilt_marks,
                hud: opts.hud,
                layout: opts.layout,
            },
        );
    }
    let mut frame = state.clone();
    let final_len = frame.community.len();
    let start_len = final_len.saturating_sub(opts.new_card_count);
    let to_reveal: Vec<_> = frame.community.split_off(start_len);

    // 第一帧：仅旧牌（可能为空）
    render(
        &frame,
        RenderOptions {
            hero_seat: opts.hero,
            log: opts.log,
            reveal_all: false,
            winners: &[],
            tilt_marks: opts.tilt_marks,
            hud: opts.hud,
            layout: opts.layout,
        },
    )?;
    thread::sleep(Duration::from_millis(opts.delay_ms));
    for c in to_reveal {
        frame.community.push(c);
        render(
            &frame,
            RenderOptions {
                hero_seat: opts.hero,
                log: opts.log,
                reveal_all: false,
                winners: &[],
                tilt_marks: opts.tilt_marks,
                hud: opts.hud,
                layout: opts.layout,
            },
        )?;
        thread::sleep(Duration::from_millis(opts.delay_ms));
    }
    Ok(())
}

/// 摊牌剧场：依次"翻"每位 in_hand 玩家的底牌（仍由 render 显示，外层只负责节奏）。
pub fn pause(ms: u64) {
    if ms > 0 {
        thread::sleep(Duration::from_millis(ms));
    }
}

/// 可被 Esc / q 中断的停顿。
/// 返回 `Ok(true)` 表示用户按 Esc/q 提前终止；`Ok(false)` 表示正常超时；
/// 其他按键被吞掉（继续等待）。
pub fn pause_interruptible(ms: u64) -> std::io::Result<bool> {
    use crossterm::event::{Event, KeyCode, poll, read};
    use std::time::Instant;
    if ms == 0 {
        return Ok(false);
    }
    let deadline = Instant::now() + Duration::from_millis(ms);
    loop {
        let now = Instant::now();
        let Some(remaining) = deadline.checked_duration_since(now) else {
            return Ok(false);
        };
        if poll(remaining)? {
            if let Event::Key(k) = read()?
                && matches!(
                    k.code,
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q')
                )
            {
                return Ok(true);
            }
            // 其他按键忽略，继续等待剩余时间
        } else {
            return Ok(false);
        }
    }
}
