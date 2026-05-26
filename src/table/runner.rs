//! 桌面循环的纯辅助逻辑。

use crate::game::state::Player;

/// 按钮位移到下一位 stack > 0 的玩家。
pub fn advance_button(players: &[Player], current: usize) -> usize {
    let n = players.len();
    for step in 1..=n {
        let i = (current + step) % n;
        if players[i].stack > 0 {
            return i;
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_busted_players() {
        let mut players = vec![
            Player::new("A", 100),
            Player::new("B", 0),
            Player::new("C", 100),
        ];
        assert_eq!(advance_button(&players, 0), 2);
        players[2].stack = 0;
        assert_eq!(advance_button(&players, 0), 0);
    }
}
