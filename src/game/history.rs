//! 简易牌局快照与回放数据结构。
//!
//! 在主循环里每次 apply_action / advance_street 之后 push 一帧；
//! `Frame` 直接保存 `HandState.clone()`（实现简单，对短回放完全够用）。

use crate::game::state::HandState;

#[derive(Debug, Clone)]
pub struct Frame {
    pub state: HandState,
    /// 该帧对应的最新若干条 log（截至该帧总长度）。
    pub log_len: usize,
}

#[derive(Debug, Clone)]
pub struct HandHistory {
    pub frames: Vec<Frame>,
    /// 本手牌的日志快照（相对本手开局）。
    pub logs: Vec<String>,
    /// 摊牌或单人胜出时的最终 log 总长度。
    pub final_log_len: usize,
    /// 最终赢家座位。
    pub winners: Vec<usize>,
}

impl HandHistory {
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            logs: Vec::new(),
            final_log_len: 0,
            winners: Vec::new(),
        }
    }

    pub fn push(&mut self, state: &HandState, log_len: usize) {
        self.frames.push(Frame {
            state: state.clone(),
            log_len,
        });
    }

    pub fn set_logs(&mut self, logs: &[String], start_log_len: usize) {
        self.logs = logs[start_log_len..].to_vec();
        for frame in &mut self.frames {
            frame.log_len = frame.log_len.saturating_sub(start_log_len);
        }
        self.final_log_len = self.final_log_len.saturating_sub(start_log_len);
    }
}

impl Default for HandHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::action::Action;
    use crate::game::betting::{advance_street, apply_action, round_closed, start_hand};
    use crate::game::state::Player;
    use rand::SeedableRng;

    #[test]
    fn frames_grow_during_a_hand() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(11);
        let players = vec![
            Player::new("A", 1000),
            Player::new("B", 1000),
            Player::new("C", 1000),
        ];
        let mut s = start_hand(players, 0, 5, 10, &mut rng);
        let mut h = HandHistory::new();
        h.push(&s, 0);
        // UTG fold
        apply_action(&mut s, 0, Action::Fold).unwrap();
        h.push(&s, 1);
        // SB call
        apply_action(&mut s, 1, Action::Call).unwrap();
        h.push(&s, 2);
        // BB check
        apply_action(&mut s, 2, Action::Check).unwrap();
        h.push(&s, 3);
        assert!(round_closed(&s));
        advance_street(&mut s);
        h.push(&s, 4);
        assert_eq!(h.frames.len(), 5);
        // stacks 总和守恒
        let totals: Vec<u64> = h
            .frames
            .iter()
            .map(|f| {
                f.state
                    .players
                    .iter()
                    .map(|p| p.stack + p.committed_total)
                    .sum::<u64>()
            })
            .collect();
        for t in &totals {
            assert_eq!(*t, totals[0]);
        }
    }
}
