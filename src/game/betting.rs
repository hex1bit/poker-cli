//! 下注轮逻辑：发牌、贴盲、动作合法性校验、轮转、街切换。

use rand::Rng;

use crate::card::shuffled_deck;
use crate::game::action::Action;
use crate::game::state::{HandState, Player, PlayerStatus, Stage};

/// 创建一手新牌：洗牌、贴盲、发底牌、定下首位行动者。
///
/// `players` 至少 2 人；`button` 是按钮位的座位下标（用于决定 SB/BB）。
pub fn start_hand<R: Rng + ?Sized>(
    players: Vec<Player>,
    button: usize,
    sb: u64,
    bb: u64,
    rng: &mut R,
) -> HandState {
    assert!(players.len() >= 2, "need ≥ 2 players");
    let n = players.len();
    let mut state = HandState {
        players,
        button,
        stage: Stage::Preflop,
        community: Vec::with_capacity(5),
        deck: shuffled_deck(rng),
        current_bet: 0,
        min_raise: bb,
        to_act: None,
        last_aggressor: None,
        sb,
        bb,
    };

    // 仅保留有筹码的活跃玩家进入本手。
    for p in &mut state.players {
        if p.stack == 0 {
            p.status = PlayerStatus::SitOut;
        } else {
            p.status = PlayerStatus::Active;
        }
        p.hole = None;
        p.committed_round = 0;
        p.committed_total = 0;
        p.acted_this_round = false;
        p.last_revealed = None;
    }

    // 发底牌（两轮发，符合真实习惯）。
    let mut active_seats: Vec<usize> = (0..n)
        .filter(|&i| state.players[i].status == PlayerStatus::Active)
        .collect();
    // 发牌顺序：从按钮位左手第一个开始
    rotate_to_first_after(&mut active_seats, button);
    for _ in 0..2 {
        for &seat in &active_seats {
            let c = state.deck.pop().expect("deck has cards");
            let p = &mut state.players[seat];
            p.hole = match p.hole {
                None => Some([c, c]),
                Some([a, _]) => Some([a, c]),
            };
        }
    }

    // 贴盲：SB / BB 位置。
    let (sb_seat, bb_seat) = blind_seats(&state.players, button);
    post_blind(&mut state, sb_seat, sb);
    post_blind(&mut state, bb_seat, bb);
    state.current_bet = bb;
    state.min_raise = bb;
    // 贴盲不算 “acted_this_round”，BB 仍保留 option。
    state.players[sb_seat].acted_this_round = false;
    state.players[bb_seat].acted_this_round = false;
    state.last_aggressor = Some(bb_seat); // 闭环参考点

    // Preflop 首位行动者 = BB 之后第一个 Active。
    state.to_act = Some(next_active_after(&state.players, bb_seat));

    state
}

/// 给定按钮位，返回 (SB, BB) 座位下标。
fn blind_seats(players: &[Player], button: usize) -> (usize, usize) {
    let active: Vec<usize> = (0..players.len())
        .map(|i| (button + i) % players.len())
        .filter(|&i| players[i].status == PlayerStatus::Active)
        .collect();
    assert!(active.len() >= 2);
    if active.len() == 2 {
        // 头对头：按钮位 = SB
        (active[0], active[1])
    } else {
        (active[1], active[2])
    }
}

fn post_blind(state: &mut HandState, seat: usize, amount: u64) {
    let p = &mut state.players[seat];
    let put = amount.min(p.stack);
    p.stack -= put;
    p.committed_round += put;
    p.committed_total += put;
    if p.stack == 0 {
        p.status = PlayerStatus::AllIn;
    }
}

fn rotate_to_first_after(seats: &mut [usize], button: usize) {
    seats.sort_unstable();
    // 找到第一个 > button 的下标位置
    let pos = seats.iter().position(|&i| i > button).unwrap_or(0);
    seats.rotate_left(pos);
}

/// 找到 `from` 之后第一个 Active 玩家（环绕），如果没有则 panic。
fn next_active_after(players: &[Player], from: usize) -> usize {
    let n = players.len();
    for step in 1..=n {
        let i = (from + step) % n;
        if players[i].status == PlayerStatus::Active {
            return i;
        }
    }
    panic!("no active player after {from}");
}

/// 给定行动者 `idx`，应用合法动作。返回 `Err` 则动作非法（不会修改状态）。
pub fn apply_action(state: &mut HandState, idx: usize, action: Action) -> Result<(), String> {
    if Some(idx) != state.to_act {
        return Err(format!("not {idx}'s turn"));
    }
    let to_call = state.to_call_for(idx);
    let p = &state.players[idx];
    if p.status != PlayerStatus::Active {
        return Err("player cannot act".into());
    }

    match action {
        Action::Fold => {
            state.players[idx].status = PlayerStatus::Folded;
            state.players[idx].acted_this_round = true;
        }
        Action::Check => {
            if to_call != 0 {
                return Err(format!("cannot check, need to call {to_call}"));
            }
            state.players[idx].acted_this_round = true;
        }
        Action::Call => {
            if to_call == 0 {
                return Err("nothing to call; use check".into());
            }
            let pay = to_call.min(state.players[idx].stack);
            commit(state, idx, pay);
            state.players[idx].acted_this_round = true;
        }
        Action::Bet(total) => {
            if state.current_bet != 0 {
                return Err("cannot bet; there is already a bet (use raise)".into());
            }
            if total < state.bb {
                return Err(format!("bet must be ≥ bb ({})", state.bb));
            }
            let player_stack = state.players[idx].stack;
            if total > player_stack {
                return Err("bet exceeds stack".into());
            }
            commit(state, idx, total);
            state.current_bet = total;
            state.min_raise = total;
            state.last_aggressor = Some(idx);
            reset_others_acted(state, idx);
            state.players[idx].acted_this_round = true;
        }
        Action::Raise(to_total) => {
            if state.current_bet == 0 {
                return Err("nothing to raise; use bet".into());
            }
            if to_total <= state.current_bet {
                return Err(format!(
                    "raise total {} must exceed current bet {}",
                    to_total, state.current_bet
                ));
            }
            let raise_delta = to_total - state.current_bet;
            // 玩家需要再投入的额度
            let need_pay = to_total - state.players[idx].committed_round;
            let player_stack = state.players[idx].stack;
            if need_pay > player_stack {
                return Err("raise exceeds stack (use all-in)".into());
            }
            if raise_delta < state.min_raise && need_pay < player_stack {
                return Err(format!(
                    "raise increment {} below min-raise {}",
                    raise_delta, state.min_raise
                ));
            }
            commit(state, idx, need_pay);
            state.current_bet = to_total;
            state.min_raise = raise_delta.max(state.min_raise);
            state.last_aggressor = Some(idx);
            reset_others_acted(state, idx);
            state.players[idx].acted_this_round = true;
        }
        Action::AllIn => {
            let stack = state.players[idx].stack;
            if stack == 0 {
                return Err("no chips to push".into());
            }
            let new_commit = state.players[idx].committed_round + stack;
            let prev_bet = state.current_bet;
            commit(state, idx, stack);
            if new_commit > prev_bet {
                let raise_delta = new_commit - prev_bet;
                state.current_bet = new_commit;
                // 即使 raise 不足 min_raise，all-in 也合法，但通常不重开下注（标准规则：
                // 不到 min_raise 的 all-in 不允许先前 caller 再次加注，简化起见我们这里仍按
                // 普通加注处理 —— 教学项目可接受）。
                state.min_raise = raise_delta.max(state.min_raise);
                state.last_aggressor = Some(idx);
                reset_others_acted(state, idx);
            }
            state.players[idx].acted_this_round = true;
        }
    }

    // 推进 to_act
    advance_to_act(state);
    Ok(())
}

fn commit(state: &mut HandState, idx: usize, delta: u64) {
    let p = &mut state.players[idx];
    let pay = delta.min(p.stack);
    p.stack -= pay;
    p.committed_round += pay;
    p.committed_total += pay;
    if p.stack == 0 {
        p.status = PlayerStatus::AllIn;
    }
}

fn reset_others_acted(state: &mut HandState, aggressor: usize) {
    for (i, p) in state.players.iter_mut().enumerate() {
        if i != aggressor && p.status == PlayerStatus::Active {
            p.acted_this_round = false;
        }
    }
}

/// 推进 `to_act`：找到下一个可行动者，或在本轮结束时清空。
fn advance_to_act(state: &mut HandState) {
    let cur = state.to_act.expect("must have current actor");

    // 早结束：只剩 ≤1 个 in_hand → 本手提前结束。
    if state.in_hand_count() <= 1 {
        state.to_act = None;
        return;
    }

    // 本轮是否封闭？
    let active_to_act: Vec<usize> = state
        .players
        .iter()
        .enumerate()
        .filter(|(_, p)| p.can_act())
        .map(|(i, _)| i)
        .collect();
    let all_matched = active_to_act
        .iter()
        .all(|&i| state.players[i].committed_round == state.current_bet);
    let all_acted = active_to_act
        .iter()
        .all(|&i| state.players[i].acted_this_round);

    if active_to_act.is_empty() || (all_matched && all_acted) {
        state.to_act = None;
        return;
    }

    // 找下一个能行动的人
    let n = state.players.len();
    for step in 1..=n {
        let i = (cur + step) % n;
        if state.players[i].can_act() {
            // 该玩家要么尚未行动，要么 commit 不足
            if !state.players[i].acted_this_round
                || state.players[i].committed_round < state.current_bet
            {
                state.to_act = Some(i);
                return;
            }
        }
    }
    state.to_act = None;
}

/// 本轮是否已封闭（无人需要继续行动）。
pub fn round_closed(state: &HandState) -> bool {
    state.to_act.is_none()
}

/// 切换到下一街：发公共牌，重置 committed_round，决定首位行动者。
///
/// 若已达 Showdown 或 Complete，不做任何事。
pub fn advance_street(state: &mut HandState) {
    if !round_closed(state) {
        return;
    }
    // 单人剩下：直接结束
    if state.in_hand_count() <= 1 {
        state.stage = Stage::Complete;
        state.to_act = None;
        return;
    }

    match state.stage {
        Stage::Preflop => deal_flop(state),
        Stage::Flop => deal_turn(state),
        Stage::Turn => deal_river(state),
        Stage::River => {
            state.stage = Stage::Showdown;
            state.to_act = None;
            return;
        }
        Stage::Showdown | Stage::Complete => return,
    }

    // 准备新一街
    state.current_bet = 0;
    state.min_raise = state.bb;
    for p in &mut state.players {
        p.committed_round = 0;
        if p.status == PlayerStatus::Active {
            p.acted_this_round = false;
        }
    }
    state.last_aggressor = None;

    // 若只剩 ≤1 人可行动（其他全 all-in），无需再下注，直接快进到 showdown。
    if state.players.iter().filter(|p| p.can_act()).count() <= 1 && state.in_hand_count() > 1 {
        // 还需要发完剩余的公共牌
        while state.community.len() < 5 {
            match state.stage {
                Stage::Flop => deal_turn(state),
                Stage::Turn => deal_river(state),
                _ => break,
            }
        }
        state.stage = Stage::Showdown;
        state.to_act = None;
        return;
    }

    // 首位行动者：按钮位之后的第一个 Active。
    let first = next_active_after(&state.players, state.button);
    state.to_act = Some(first);
}

fn deal_flop(state: &mut HandState) {
    // 烧一张
    state.deck.pop();
    for _ in 0..3 {
        state.community.push(state.deck.pop().unwrap());
    }
    state.stage = Stage::Flop;
}

fn deal_turn(state: &mut HandState) {
    state.deck.pop();
    state.community.push(state.deck.pop().unwrap());
    state.stage = Stage::Turn;
}

fn deal_river(state: &mut HandState) {
    state.deck.pop();
    state.community.push(state.deck.pop().unwrap());
    state.stage = Stage::River;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use std::collections::HashSet;

    fn fresh(players: usize) -> HandState {
        let mut rng = rand::rngs::StdRng::seed_from_u64(99);
        let plist: Vec<Player> = (0..players)
            .map(|i| Player::new(format!("P{i}"), 1000))
            .collect();
        start_hand(plist, 0, 5, 10, &mut rng)
    }

    fn seen_cards(state: &HandState) -> Vec<u8> {
        let mut cards = Vec::new();
        for p in &state.players {
            if let Some(h) = p.hole {
                cards.push(h[0].index());
                cards.push(h[1].index());
            }
        }
        cards.extend(state.community.iter().map(|c| c.index()));
        cards.extend(state.deck.iter().map(|c| c.index()));
        cards
    }

    #[test]
    fn start_hand_deals_exactly_two_cards_per_active_player() {
        let s = fresh(6);
        assert_eq!(s.deck.len(), 52 - 6 * 2);
        for p in &s.players {
            assert!(p.hole.is_some());
        }
    }

    #[test]
    fn dealt_cards_and_deck_remain_unique() {
        let mut s = fresh(6);
        let cards = seen_cards(&s);
        let unique: HashSet<_> = cards.iter().copied().collect();
        assert_eq!(cards.len(), 52);
        assert_eq!(unique.len(), 52);

        while let Some(seat) = s.to_act {
            let action = if s.to_call_for(seat) == 0 {
                Action::Check
            } else {
                Action::Call
            };
            apply_action(&mut s, seat, action).unwrap();
        }
        advance_street(&mut s);
        assert_eq!(s.community.len(), 3);
        let cards = seen_cards(&s);
        let unique: HashSet<_> = cards.iter().copied().collect();
        assert_eq!(cards.len(), 51);
        assert_eq!(unique.len(), 51);
        assert_eq!(s.deck.len(), 52 - 12 - 1 - 3);
    }

    #[test]
    fn blinds_posted() {
        let s = fresh(3);
        // button=0, SB=1, BB=2
        assert_eq!(s.players[1].committed_round, 5);
        assert_eq!(s.players[2].committed_round, 10);
        assert_eq!(s.current_bet, 10);
        // UTG (idx 0) acts first preflop in 3-handed
        assert_eq!(s.to_act, Some(0));
    }

    #[test]
    fn heads_up_button_is_sb() {
        let s = fresh(2);
        // button=0, SB=0, BB=1; preflop SB(=button) acts first
        assert_eq!(s.players[0].committed_round, 5);
        assert_eq!(s.players[1].committed_round, 10);
        assert_eq!(s.to_act, Some(0));
    }

    #[test]
    fn ten_handed_preflop_reaches_button_player() {
        let mut s = fresh(10);
        let mut order = Vec::new();
        while let Some(seat) = s.to_act {
            order.push(seat);
            let action = if s.to_call_for(seat) == 0 {
                Action::Check
            } else {
                Action::Call
            };
            apply_action(&mut s, seat, action).unwrap();
        }
        assert_eq!(order, vec![3, 4, 5, 6, 7, 8, 9, 0, 1, 2]);
        assert!(round_closed(&s));
    }

    #[test]
    fn fold_call_check_through_flop() {
        // 3-handed: UTG fold, SB call, BB check → 到 flop
        let mut s = fresh(3);
        apply_action(&mut s, 0, Action::Fold).unwrap();
        apply_action(&mut s, 1, Action::Call).unwrap();
        apply_action(&mut s, 2, Action::Check).unwrap();
        assert!(round_closed(&s));
        advance_street(&mut s);
        assert_eq!(s.stage, Stage::Flop);
        assert_eq!(s.community.len(), 3);
        // 首位行动者：button=0 之后第一个 Active = SB(idx1)
        assert_eq!(s.to_act, Some(1));
        assert_eq!(s.current_bet, 0);
    }

    #[test]
    fn raise_reopens_action() {
        let mut s = fresh(3);
        apply_action(&mut s, 0, Action::Raise(30)).unwrap();
        assert_eq!(s.current_bet, 30);
        // SB 与 BB 都必须再次行动
        apply_action(&mut s, 1, Action::Call).unwrap();
        apply_action(&mut s, 2, Action::Call).unwrap();
        assert!(round_closed(&s));
        // 所有人都付到 30
        for i in 0..3 {
            assert_eq!(s.players[i].committed_round, 30);
        }
    }

    #[test]
    fn min_raise_enforced() {
        let mut s = fresh(3);
        apply_action(&mut s, 0, Action::Raise(30)).unwrap(); // +20 (min_raise becomes 20)
        // SB 想加注 35（增量 5），不足 min_raise=20
        let err = apply_action(&mut s, 1, Action::Raise(35));
        assert!(err.is_err());
    }

    #[test]
    fn fold_to_one_ends_hand() {
        let mut s = fresh(3);
        apply_action(&mut s, 0, Action::Raise(40)).unwrap();
        apply_action(&mut s, 1, Action::Fold).unwrap();
        apply_action(&mut s, 2, Action::Fold).unwrap();
        assert!(round_closed(&s));
        advance_street(&mut s);
        assert_eq!(s.stage, Stage::Complete);
    }

    #[test]
    fn allin_short_stack_call() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let mut players = vec![
            Player::new("A", 1000),
            Player::new("B", 1000),
            Player::new("C", 80), // 短码
        ];
        let mut s = start_hand(players.split_off(0), 0, 5, 10, &mut rng);
        // button=0; SB=1, BB=2 (commit 10, stack=70)
        // UTG (0) raise to 200
        apply_action(&mut s, 0, Action::Raise(200)).unwrap();
        // SB fold
        apply_action(&mut s, 1, Action::Fold).unwrap();
        // BB call → 只能投入剩余 70 (commit 总 80)
        apply_action(&mut s, 2, Action::Call).unwrap();
        assert_eq!(s.players[2].status, PlayerStatus::AllIn);
        assert_eq!(s.players[2].committed_round, 80);
        assert_eq!(s.players[2].stack, 0);
        assert!(round_closed(&s));
    }
}
