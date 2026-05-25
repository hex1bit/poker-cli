//! 公共牌发牌动画 + 节奏控制。
//!
//! 在 advance_street 完成后调用，逐张揭示新发的公共牌；
//! 通过 clone HandState 短暂"撤回"已发的牌实现伪动画，最终保持原 state 不变。

use std::thread;
use std::time::Duration;

use crate::game::state::HandState;
use crate::ui::render::render;

/// 逐张揭示最新发出的 `new_card_count` 张公共牌。
///
/// `delay_ms == 0` 时退化为立即整版重绘一次。
pub fn animate_deal_community(
    state: &HandState,
    hero: Option<usize>,
    log: &[String],
    tilt_marks: &[bool],
    new_card_count: usize,
    delay_ms: u64,
) -> std::io::Result<()> {
    if delay_ms == 0 || new_card_count == 0 {
        return render(state, hero, log, false, &[], tilt_marks);
    }
    let mut frame = state.clone();
    let final_len = frame.community.len();
    let start_len = final_len.saturating_sub(new_card_count);
    let to_reveal: Vec<_> = frame.community.split_off(start_len);

    // 第一帧：仅旧牌（可能为空）
    render(&frame, hero, log, false, &[], tilt_marks)?;
    thread::sleep(Duration::from_millis(delay_ms));
    for c in to_reveal {
        frame.community.push(c);
        render(&frame, hero, log, false, &[], tilt_marks)?;
        thread::sleep(Duration::from_millis(delay_ms));
    }
    Ok(())
}

/// 摊牌剧场：依次"翻"每位 in_hand 玩家的底牌（仍由 render 显示，外层只负责节奏）。
pub fn pause(ms: u64) {
    if ms > 0 {
        thread::sleep(Duration::from_millis(ms));
    }
}
