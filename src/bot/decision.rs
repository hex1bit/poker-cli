//! Bot 决策核心：把 equity / pot_odds / 性格参数组合成具体 `Action`。

use rand::Rng;

use crate::bot::mood::Mood;
use crate::bot::personality::Personality;
use crate::equity::{mc_equity, preflop_strength};
use crate::game::action::Action;
use crate::game::state::{HandState, PlayerStatus, Stage};

/// 蒙特卡洛采样次数。Postflop 决策时使用。
const MC_ITERS: usize = 600;

/// 让一个 bot 在当前状态下决定动作（不带 mood，等价于 mood = default）。
///
/// 必须保证 `state.to_act == Some(seat)` 且该玩家为 Active。
pub fn decide<R: Rng + ?Sized>(
    state: &HandState,
    seat: usize,
    persona: Personality,
    rng: &mut R,
) -> Action {
    decide_with_mood(state, seat, persona, &Mood::default(), rng)
}

/// 带情绪的决策：tilt 上升时 effective aggression / vpip / bluff 也提升。
pub fn decide_with_mood<R: Rng + ?Sized>(
    state: &HandState,
    seat: usize,
    persona: Personality,
    mood: &Mood,
    rng: &mut R,
) -> Action {
    let hole = state.players[seat]
        .hole
        .expect("bot needs hole cards to decide");
    let to_call = state.to_call_for(seat);
    let pot = state.total_pot();
    let stack = state.players[seat].stack;
    let opponents = state
        .players
        .iter()
        .enumerate()
        .filter(|(i, p)| *i != seat && p.is_in_hand())
        .count()
        .max(1);

    // mood 注入：tilt 上升 → 更激进、更松、更爱 bluff
    let tilt = mood.tilt;
    let eff_aggression = (persona.aggression * (1.0 + tilt)).min(6.0);
    let eff_vpip = (persona.vpip + tilt * 0.10).clamp(0.0, 1.0);
    let eff_pfr = (persona.pfr + tilt * 0.10).clamp(0.0, 1.0);
    let eff_bluff = (persona.bluff_freq + tilt * 0.10).clamp(0.0, 1.0);

    // 1) 计算 equity。
    let equity = if state.stage == Stage::Preflop {
        preflop_strength(hole)
    } else {
        mc_equity(hole, &state.community, opponents, MC_ITERS, rng)
    };

    // 2) 底池赔率（call 需要的最小 equity）。
    let pot_odds = if to_call == 0 {
        0.0
    } else {
        to_call as f64 / (pot + to_call) as f64
    };
    let edge = equity - pot_odds;

    // 3) 位置加权：晚位 (button 附近) 更松。
    let position_bonus = position_weight(state, seat, persona.position_aware);

    // ===== Preflop 入池决定 =====
    if state.stage == Stage::Preflop {
        let effective_vpip = (eff_vpip + position_bonus * 0.10).clamp(0.0, 1.0);
        let raise_threshold = preflop_strength_threshold(eff_pfr);
        let call_threshold = preflop_strength_threshold(effective_vpip);

        // 自由 BB option：to_call == 0
        if to_call == 0 {
            if equity >= raise_threshold || rng.r#gen::<f64>() < eff_bluff * 0.5 {
                return raise_size(state, seat, persona, equity, eff_aggression, rng);
            }
            return Action::Check;
        }

        // 需要付钱才能进入
        if equity >= raise_threshold {
            // 强牌：偶尔慢玩
            if rng.r#gen::<f64>() < persona.slowplay_freq * 0.5 {
                return Action::Call;
            }
            return raise_size(state, seat, persona, equity, eff_aggression, rng);
        }
        if equity >= call_threshold {
            // 中等：跟注
            return safe_call_or_fold(state, seat, to_call);
        }
        // 弱牌：偶尔诈唬，否则弃
        if rng.r#gen::<f64>() < eff_bluff * 0.5 && stack > to_call * 3 {
            return raise_size(state, seat, persona, equity, eff_aggression, rng);
        }
        return Action::Fold;
    }

    // ===== Postflop 决策 =====
    let is_aggressor_last_street = state.last_aggressor == Some(seat);
    let scary_board = board_scariness(&state.community);

    // 没人下注 → 选择 check / bet
    if to_call == 0 {
        // 强牌
        if equity > 0.65 {
            if rng.r#gen::<f64>() < persona.slowplay_freq {
                return Action::Check;
            }
            return open_bet(state, seat, persona, equity, eff_aggression, rng);
        }
        // 中等牌
        if equity > 0.45 {
            // 多人池子要更保守
            if opponents >= 3 && equity < 0.55 {
                return Action::Check;
            }
            // value bet 频率随 aggression 增长
            let bet_p = (0.35 * eff_aggression).clamp(0.0, 0.9);
            if rng.r#gen::<f64>() < bet_p {
                return open_bet(state, seat, persona, equity, eff_aggression, rng);
            }
            return Action::Check;
        }
        // 弱牌：cbet 或 bluff
        let cbet_chance = if is_aggressor_last_street {
            persona.cbet_freq
        } else {
            eff_bluff * (1.0 + scary_board)
        };
        if rng.r#gen::<f64>() < cbet_chance {
            return open_bet(state, seat, persona, equity.max(0.30), eff_aggression, rng);
        }
        return Action::Check;
    }

    // 有人下注，需要决定 fold/call/raise
    // 强牌
    if edge > 0.20 {
        if rng.r#gen::<f64>() < persona.slowplay_freq * 0.5 {
            return safe_call_or_fold(state, seat, to_call);
        }
        return raise_size(state, seat, persona, equity, eff_aggression, rng);
    }
    // 有正期望
    if edge > 0.05 {
        // aggression 大 → 偏向加注
        let raise_p = (0.30 * eff_aggression).clamp(0.0, 0.85);
        if rng.r#gen::<f64>() < raise_p {
            return raise_size(state, seat, persona, equity, eff_aggression, rng);
        }
        return safe_call_or_fold(state, seat, to_call);
    }
    // 边际：偶尔跟
    if edge > -0.05 {
        return safe_call_or_fold(state, seat, to_call);
    }
    // 烂牌：偶尔诈唬加注
    if rng.r#gen::<f64>() < eff_bluff * (0.5 + 0.5 * scary_board) {
        return raise_size(state, seat, persona, 0.45, eff_aggression, rng);
    }
    Action::Fold
}

/// 由 `pfr` / `vpip` 估算对应的 preflop 强度阈值。
/// 简化：阈值 = 1 - frequency。频率越高，阈值越低。
fn preflop_strength_threshold(freq: f64) -> f64 {
    (1.0 - freq).clamp(0.30, 0.95)
}

/// 决定加注金额（既可能是 Bet，也可能是 Raise）。
fn raise_size<R: Rng + ?Sized>(
    state: &HandState,
    seat: usize,
    _persona: Personality,
    equity: f64,
    eff_aggression: f64,
    _rng: &mut R,
) -> Action {
    let pot = state.total_pot().max(state.bb);
    // 基础尺度：equity 越高/aggression 越高，下注越大。
    let scale = ((equity - 0.35).max(0.0) * 1.5 + 0.5) * (0.7 + 0.3 * eff_aggression);
    let mut target_total: u64 = if state.current_bet == 0 {
        // open bet
        ((pot as f64) * scale.clamp(0.4, 1.2)) as u64
    } else {
        // raise to total
        let raise_size = ((pot as f64) * scale.clamp(0.5, 1.5)) as u64;
        state.current_bet + raise_size
    };
    target_total = target_total.max(state.bb);

    // 校验合法性：必须 ≥ current_bet + min_raise（除非全押）
    if state.current_bet > 0 {
        let min_total = state.current_bet + state.min_raise;
        if target_total < min_total {
            target_total = min_total;
        }
    } else if target_total < state.bb {
        target_total = state.bb;
    }

    // 需要付的金额
    let need_pay = target_total.saturating_sub(state.players[seat].committed_round);
    let stack = state.players[seat].stack;
    if need_pay >= stack {
        return Action::AllIn;
    }
    if state.current_bet == 0 {
        Action::Bet(target_total)
    } else {
        Action::Raise(target_total)
    }
}

/// 没有 "开桌下注" 选项时（已有 bet/raise）：决定 call / fold。
fn safe_call_or_fold(state: &HandState, seat: usize, to_call: u64) -> Action {
    let stack = state.players[seat].stack;
    if to_call == 0 {
        Action::Check
    } else if to_call >= stack {
        Action::AllIn
    } else {
        Action::Call
    }
}

/// open bet 包装。
fn open_bet<R: Rng + ?Sized>(
    state: &HandState,
    seat: usize,
    persona: Personality,
    equity: f64,
    eff_aggression: f64,
    rng: &mut R,
) -> Action {
    raise_size(state, seat, persona, equity, eff_aggression, rng)
}

/// 位置加权 ∈ [-0.1, 0.1]：越靠近按钮位返回越大。
fn position_weight(state: &HandState, seat: usize, awareness: f64) -> f64 {
    let n = state.players.len();
    let active: Vec<usize> = (0..n)
        .filter(|&i| state.players[i].status != PlayerStatus::SitOut)
        .collect();
    if active.is_empty() {
        return 0.0;
    }
    // 距离按钮位的 “逆时针距离”：button 自身=0, button-1=1, ...
    let dist = (state.button + n - seat) % n;
    let rel = dist as f64 / n as f64; // 0..~1
    // 0 (button) → +0.10 * awareness; 远离 → 负
    let w = (0.10 - rel * 0.20) * awareness;
    w.clamp(-0.10, 0.10)
}

/// 板面“可怕度” ∈ [0,1]：包含同花、顺子可能性的粗略指标。
fn board_scariness(board: &[crate::card::Card]) -> f64 {
    if board.len() < 3 {
        return 0.0;
    }
    let mut suits = [0u8; 4];
    for c in board {
        suits[c.suit().as_u8() as usize] += 1;
    }
    let max_suit = *suits.iter().max().unwrap();
    let flush_threat = match max_suit {
        4..=5 => 0.5,
        3 => 0.25,
        _ => 0.0,
    };
    // 简单连通度：rank 集合排序后看最大窗口内 5 个 rank 中有多少
    let mut ranks: Vec<u8> = board.iter().map(|c| c.rank().as_u8()).collect();
    ranks.sort_unstable();
    ranks.dedup();
    let mut max_in_window = 0;
    for top in 4..=12u8 {
        let lo = top.saturating_sub(4);
        let cnt = ranks.iter().filter(|&&r| r >= lo && r <= top).count();
        if cnt > max_in_window {
            max_in_window = cnt;
        }
    }
    let straight_threat = (max_in_window as f64 - 2.0).max(0.0) * 0.15;
    (flush_threat + straight_threat).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Card, Rank, Suit};
    use crate::game::betting::{apply_action, start_hand};
    use crate::game::state::Player;
    use rand::SeedableRng;

    fn aa(seat: usize) -> [Card; 2] {
        let s = if seat == 0 {
            (Suit::Hearts, Suit::Spades)
        } else {
            (Suit::Clubs, Suit::Diamonds)
        };
        [Card::new(Rank::Ace, s.0), Card::new(Rank::Ace, s.1)]
    }

    #[test]
    fn rock_folds_trash_preflop_facing_raise() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let mut s = start_hand(
            vec![Player::new("A", 1000), Player::new("B", 1000), Player::new("C", 1000)],
            0,
            5,
            10,
            &mut rng,
        );
        // 强制 seat 0 是 ROCK，给 trash 7-2 offsuit；面对 raise
        s.players[0].hole = Some([
            Card::new(Rank::Seven, Suit::Hearts),
            Card::new(Rank::Two, Suit::Diamonds),
        ]);
        // 模拟前面有人 raise to 60
        // 当前 to_act 应该是 0（3-handed: UTG=0）
        // 手动制造一个 “面对加注” 的情况：先让 0 fold (no), 我们改方式 —— 直接调高 current_bet
        s.current_bet = 60;
        // ROCK 应弃
        let a = decide(&s, 0, Personality::ROCK, &mut rng);
        assert_eq!(a, Action::Fold);
    }

    #[test]
    fn maniac_raises_premium() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(2);
        let mut s = start_hand(
            vec![Player::new("A", 1000), Player::new("B", 1000), Player::new("C", 1000)],
            0,
            5,
            10,
            &mut rng,
        );
        s.players[0].hole = Some(aa(0));
        let a = decide(&s, 0, Personality::MANIAC, &mut rng);
        // AA 几乎肯定 raise 或 all-in
        assert!(matches!(a, Action::Raise(_) | Action::AllIn));
    }

    #[test]
    fn fish_calls_with_marginal() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let mut s = start_hand(
            vec![Player::new("A", 1000), Player::new("B", 1000), Player::new("C", 1000)],
            0,
            5,
            10,
            &mut rng,
        );
        // 给 Fish 一个边际牌 KJ 同花
        s.players[0].hole = Some([
            Card::new(Rank::King, Suit::Hearts),
            Card::new(Rank::Jack, Suit::Hearts),
        ]);
        s.current_bet = 30; // 有人 raise 了
        let a = decide(&s, 0, Personality::FISH, &mut rng);
        // Fish vpip 高，应至少 call（也可能因为强度高而 raise）
        assert!(matches!(a, Action::Call | Action::Raise(_) | Action::AllIn));
        let _ = apply_action; // silence unused
    }

    #[test]
    fn bot_vs_bot_runs_to_completion() {
        // 跑一手 6 个 bot vs bot, 验证不 panic 且 stage 抵达 Complete/Showdown 且筹码守恒
        let mut rng = rand::rngs::StdRng::seed_from_u64(101);
        let players: Vec<Player> = (0..6)
            .map(|i| Player::new(format!("Bot{i}"), 1000))
            .collect();
        let initial_total: u64 = players.iter().map(|p| p.stack).sum();
        let personas = Personality::PRESETS;

        let mut s = start_hand(players, 0, 5, 10, &mut rng);
        use crate::game::betting::{advance_street, round_closed};
        use crate::game::showdown::settle;

        let mut safety = 0usize;
        while s.stage != Stage::Complete && s.stage != Stage::Showdown {
            if let Some(seat) = s.to_act {
                let p = personas[seat % personas.len()];
                let a = decide(&s, seat, p, &mut rng);
                apply_action(&mut s, seat, a).expect("bot picks legal action");
            } else if round_closed(&s) {
                advance_street(&mut s);
            }
            safety += 1;
            assert!(safety < 500, "infinite loop?");
        }
        if s.stage == Stage::Showdown {
            settle(&mut s);
        } else {
            // 单人剩下 —— 直接结算
            settle(&mut s);
        }
        let final_total: u64 = s.players.iter().map(|p| p.stack).sum();
        assert_eq!(final_total, initial_total, "chip conservation broken");
    }
}
