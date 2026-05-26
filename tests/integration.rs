//! 端到端集成测试：用脚本动作驱动一手 6 人桌跑通，验证筹码守恒。

use poker::bot::{Personality, decide};
use poker::game::action::Action;
use poker::game::betting::{advance_street, apply_action, round_closed, start_hand};
use poker::game::showdown::settle;
use poker::game::state::{Player, Stage};
use rand::SeedableRng;

#[test]
fn six_bots_play_30_hands_chip_conservation() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(2024);
    let mut players: Vec<Player> = Personality::PRESETS
        .iter()
        .map(|p| Player::new(p.label.to_string(), 1500))
        .collect();
    let initial_total: u64 = players.iter().map(|p| p.stack).sum();
    let mut button = 0usize;

    for _hand in 0..30 {
        // 必须 ≥ 2 玩家有筹码，否则结束
        let alive: Vec<usize> = (0..players.len())
            .filter(|&i| players[i].stack > 0)
            .collect();
        if alive.len() < 2 {
            break;
        }
        // 旋按钮
        button = next_alive_after(&players, button);

        let mut state = start_hand(players.clone(), button, 5, 10, &mut rng);
        let mut steps = 0usize;
        while state.stage != Stage::Complete && state.stage != Stage::Showdown {
            if let Some(seat) = state.to_act {
                let p = Personality::PRESETS[seat % Personality::PRESETS.len()];
                let a = decide(&state, seat, p, &mut rng);
                apply_action(&mut state, seat, a).expect("legal");
            } else if round_closed(&state) {
                advance_street(&mut state);
            } else {
                panic!("stuck");
            }
            steps += 1;
            assert!(steps < 1000);
        }
        settle(&mut state);
        // 同步回 players（保留名称，更新 stack）
        for (i, p) in state.players.iter().enumerate() {
            players[i].stack = p.stack;
        }
        // 守恒
        let total: u64 = players.iter().map(|p| p.stack).sum();
        assert_eq!(total, initial_total, "chip conservation broken");
    }
}

fn next_alive_after(players: &[Player], cur: usize) -> usize {
    let n = players.len();
    for step in 1..=n {
        let i = (cur + step) % n;
        if players[i].stack > 0 {
            return i;
        }
    }
    cur
}

#[test]
fn scripted_human_seat_completes_hand() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    let mut players: Vec<Player> = vec![Player::new("YOU", 1500)];
    for p in Personality::PRESETS.iter().take(3) {
        players.push(Player::new(p.label.to_string(), 1500));
    }
    let mut state = start_hand(players, 0, 5, 10, &mut rng);
    // 真人脚本：永远 fold（除非已经过牌轮）
    let hero = 0usize;

    while state.stage != Stage::Complete && state.stage != Stage::Showdown {
        if let Some(seat) = state.to_act {
            let action = if seat == hero {
                if state.to_call_for(seat) == 0 {
                    Action::Check
                } else {
                    Action::Fold
                }
            } else {
                let p = Personality::PRESETS[(seat - 1) % 6];
                decide(&state, seat, p, &mut rng)
            };
            apply_action(&mut state, seat, action).expect("legal");
        } else if round_closed(&state) {
            advance_street(&mut state);
        }
    }
    settle(&mut state);
    let total: u64 = state.players.iter().map(|p| p.stack).sum();
    assert_eq!(total, 4 * 1500);
}
