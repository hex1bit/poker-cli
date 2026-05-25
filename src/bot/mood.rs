//! Bot 情绪 / Tilt 跟踪。
//!
//! 每位 bot 维护最近若干手的盈亏窗口；连输 ≥3 手后开始累积 tilt，
//! 让决策层把 aggression / vpip / bluff_freq 临时上调。

use std::collections::VecDeque;

const WINDOW: usize = 5;
/// tilt 进入阈值（连续亏损手数）。
const TILT_LOSS_THRESHOLD: usize = 3;

#[derive(Debug, Clone, Default)]
pub struct Mood {
    pub recent_results: VecDeque<i64>,
    /// 0..1，0 = 平稳，1 = 完全 on tilt。
    pub tilt: f64,
}

impl Mood {
    pub fn new() -> Self {
        Self::default()
    }

    /// 一手结算后调用。`delta` = 净盈亏；`tilt_factor` 来自 Personality。
    pub fn after_hand(&mut self, delta: i64, tilt_factor: f64) {
        self.recent_results.push_back(delta);
        if self.recent_results.len() > WINDOW {
            self.recent_results.pop_front();
        }
        // 末尾起连续亏损手数（赢了一手就重置）
        let trailing_losses = self
            .recent_results
            .iter()
            .rev()
            .take_while(|&&d| d < 0)
            .count();
        let target = if trailing_losses >= TILT_LOSS_THRESHOLD {
            ((trailing_losses - TILT_LOSS_THRESHOLD + 1) as f64) * 0.30
        } else {
            0.0
        };
        // 平滑收敛 + 个性 tilt_factor 作为系数
        self.tilt = (self.tilt * 0.7 + target * 0.3 * (0.5 + tilt_factor)).clamp(0.0, 1.0);
        // 赢钱时额外衰减
        if delta > 0 {
            self.tilt *= 0.5;
        }
    }

    /// 当前 tilt 是否够明显（用于触发台词与 UI 高亮）。
    pub fn is_tilted(&self) -> bool {
        self.tilt > 0.30
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilt_rises_after_three_losses() {
        let mut m = Mood::new();
        m.after_hand(-50, 0.30);
        m.after_hand(-30, 0.30);
        m.after_hand(-20, 0.30);
        assert!(m.tilt > 0.0, "tilt should be > 0 after 3 losses");
    }

    #[test]
    fn tilt_decays_after_win() {
        let mut m = Mood::new();
        m.after_hand(-10, 0.30);
        m.after_hand(-10, 0.30);
        m.after_hand(-10, 0.30);
        let t1 = m.tilt;
        m.after_hand(200, 0.30);
        assert!(m.tilt < t1, "tilt should decay after winning hand");
    }

    #[test]
    fn no_tilt_when_winning() {
        let mut m = Mood::new();
        for _ in 0..5 {
            m.after_hand(100, 0.30);
        }
        assert_eq!(m.tilt, 0.0);
    }

    #[test]
    fn high_tilt_factor_amplifies() {
        let mut m_low = Mood::new();
        let mut m_high = Mood::new();
        for _ in 0..4 {
            m_low.after_hand(-50, 0.10);
            m_high.after_hand(-50, 1.0);
        }
        assert!(m_high.tilt > m_low.tilt);
    }
}
