//! 摊牌与边池结算。
//!
//! 给定每个玩家的 `committed_total` 与 hole cards / status，根据全押分层
//! 切分主池 + 多个边池，并把每个池子按牌力分配给胜者（平分余下散筹给最靠近按钮位的人）。

use crate::card::Card;
use crate::game::state::{HandState, PlayerStatus};
use crate::hand_eval::{HandRank, evaluate_7};

/// 把一手牌的池子分配回 `players[i].stack`。
///
/// 调用者应在 `Stage::Complete` 或 `Stage::Showdown` 时调用。
/// 返回每个玩家本手净盈亏 `delta[i] = received - committed_total`。
pub fn settle(state: &mut HandState) -> Vec<i64> {
    let n = state.players.len();
    let mut received = vec![0u64; n];

    // 1) 把所有玩家按 committed_total 升序排（含 fold 的人）。
    let mut contributions: Vec<(usize, u64)> = state
        .players
        .iter()
        .enumerate()
        .map(|(i, p)| (i, p.committed_total))
        .collect();
    contributions.sort_by_key(|&(_, c)| c);

    // 2) 逐层削峰：每一层抽取一个池子。
    let mut remaining = contributions.clone();
    let mut prev_cap = 0u64;
    let mut pots: Vec<(u64, Vec<usize>)> = Vec::new(); // (size, eligible)

    while !remaining.is_empty() {
        // 当前层的 cap = 当前最小 commit
        let cap = remaining[0].1;
        if cap == prev_cap {
            // 没有更高的层，剩下的全是已经分配的
            remaining.remove(0);
            continue;
        }
        let layer = cap - prev_cap;
        // 每位“还在 remaining 里”的玩家贡献 layer
        let pot_size = layer * remaining.len() as u64;
        // 还在牌局里（未弃牌）且 commit 至少到 cap 的玩家为该池资格者
        let eligible: Vec<usize> = remaining
            .iter()
            .filter(|&&(i, _)| state.players[i].status != PlayerStatus::Folded)
            .map(|&(i, _)| i)
            .collect();
        if pot_size > 0 {
            pots.push((pot_size, eligible));
        }
        prev_cap = cap;
        // 把所有 commit == cap 的玩家移出 remaining（他们的全部 commit 已经在前面累加）
        remaining.retain(|&(_, c)| c > cap);
    }

    // 3) 每个池子选出最优牌力的玩家分配。
    let board: &[Card] = &state.community;
    let rank_cache: Vec<Option<HandRank>> = (0..n)
        .map(|i| {
            if state.players[i].status == PlayerStatus::Folded {
                None
            } else if state.community.len() < 5 {
                // 摊牌前不应调用 settle；保护性返回 None 让该玩家无法赢取。
                None
            } else {
                let h = state.players[i].hole?;
                Some(evaluate_7(&[
                    h[0], h[1], board[0], board[1], board[2], board[3], board[4],
                ]))
            }
        })
        .collect();

    for (pot_size, eligible) in pots {
        if eligible.is_empty() {
            continue;
        }
        // 若只剩 1 名 eligible（其他都 fold 了）或者牌还没发完到 showdown，则直接给他。
        if eligible.len() == 1 {
            received[eligible[0]] += pot_size;
            continue;
        }
        // 在 eligible 中找出最强 hand rank
        let mut best: Option<HandRank> = None;
        for &i in &eligible {
            if let Some(r) = rank_cache[i] {
                best = Some(match best {
                    None => r,
                    Some(cur) => {
                        if r > cur {
                            r
                        } else {
                            cur
                        }
                    }
                });
            }
        }
        let Some(best) = best else {
            // 无人可评估牌力（公共牌不全）—— 退化：均分给 eligible。
            split_evenly(&mut received, pot_size, &eligible, state.button);
            continue;
        };
        let winners: Vec<usize> = eligible
            .iter()
            .copied()
            .filter(|&i| rank_cache[i] == Some(best))
            .collect();
        split_evenly(&mut received, pot_size, &winners, state.button);
    }

    // 4) 写回 stack 并计算 delta。
    let mut delta = Vec::with_capacity(n);
    for (i, &amount) in received.iter().enumerate() {
        state.players[i].stack += amount;
        let d = amount as i64 - state.players[i].committed_total as i64;
        delta.push(d);
    }
    delta
}

/// 把 `pot_size` 平分给 `winners`；余数按 “按钮位之后第一个胜者” 优先获得 1 chip 直至分完。
fn split_evenly(received: &mut [u64], pot_size: u64, winners: &[usize], button: usize) {
    let n_w = winners.len() as u64;
    let base = pot_size / n_w;
    let mut rem = pot_size - base * n_w;
    for &w in winners {
        received[w] += base;
    }
    if rem == 0 {
        return;
    }
    // 按按钮位之后的顺序依次发放 1 chip
    let n = received.len();
    let mut order: Vec<usize> = winners.to_vec();
    order.sort_by_key(|&i| ((i + n - button) % n) as i64);
    for &w in &order {
        if rem == 0 {
            break;
        }
        received[w] += 1;
        rem -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Card, Rank, Suit};
    use crate::game::state::Player;

    fn c(r: Rank, s: Suit) -> Card {
        Card::new(r, s)
    }

    fn mk_state(
        commits_and_holes: Vec<(u64, Option<[Card; 2]>, PlayerStatus)>,
        board: Vec<Card>,
    ) -> HandState {
        let mut players = Vec::new();
        for (i, (commit, hole, status)) in commits_and_holes.into_iter().enumerate() {
            let mut p = Player::new(format!("P{i}"), 0);
            p.committed_total = commit;
            p.hole = hole;
            p.status = status;
            players.push(p);
        }
        HandState {
            players,
            button: 0,
            stage: crate::game::state::Stage::Showdown,
            community: board,
            deck: vec![],
            current_bet: 0,
            min_raise: 10,
            to_act: None,
            last_aggressor: None,
            sb: 5,
            bb: 10,
        }
    }

    #[test]
    fn single_pot_winner_takes_all() {
        // 3 玩家各投 100；A 拿到最佳牌
        let board = vec![
            c(Rank::Two, Suit::Hearts),
            c(Rank::Five, Suit::Diamonds),
            c(Rank::Nine, Suit::Clubs),
            c(Rank::King, Suit::Spades),
            c(Rank::Three, Suit::Hearts),
        ];
        let a_hole = [c(Rank::Ace, Suit::Hearts), c(Rank::Ace, Suit::Spades)];
        let b_hole = [c(Rank::Two, Suit::Spades), c(Rank::Two, Suit::Diamonds)];
        let c_hole = [c(Rank::Seven, Suit::Hearts), c(Rank::Eight, Suit::Diamonds)];
        let mut s = mk_state(
            vec![
                (100, Some(a_hole), PlayerStatus::Active),
                (100, Some(b_hole), PlayerStatus::Active),
                (100, Some(c_hole), PlayerStatus::Active),
            ],
            board,
        );
        let _ = settle(&mut s);
        // A 应该收到 300（虽然 B 的 222 是三条，但 A 的 AA+22 是 two pair 不如 trips；
        // wait — B 的 22 + board 22? 没有：board 是 2 5 9 K 3，只有一张 2。
        // 所以 B 是 三条 222 (hole 22 + board 2)。三条 > AA。
        // 重新设计：让 A 拿到 hidden monster
        // 由于上面计算 B 是 trips 2's, 比 AA 强。结果应该是 B 赢。
        assert_eq!(s.players[1].stack, 300);
        assert_eq!(s.players[0].stack, 0);
    }

    #[test]
    fn side_pot_short_stack() {
        // 三人入池：A 全押 100，B/C 各投 300。
        // A 只 eligible 主池 (100*3=300)；边池 (200*2=400) 仅 B/C 争夺。
        // 让 A 牌力最强 (赢主池)，B 强于 C (赢边池)。
        let board = vec![
            c(Rank::Two, Suit::Hearts),
            c(Rank::Three, Suit::Diamonds),
            c(Rank::Four, Suit::Clubs),
            c(Rank::Nine, Suit::Spades),
            c(Rank::Jack, Suit::Hearts),
        ];
        // A: pocket aces -> two pair AA (best)
        let a_hole = [c(Rank::Ace, Suit::Hearts), c(Rank::Ace, Suit::Spades)];
        // B: pocket kings -> two pair KK
        let b_hole = [c(Rank::King, Suit::Hearts), c(Rank::King, Suit::Spades)];
        // C: 7 5 offsuit -> high card
        let c_hole = [c(Rank::Seven, Suit::Hearts), c(Rank::Five, Suit::Diamonds)];
        let mut s = mk_state(
            vec![
                (100, Some(a_hole), PlayerStatus::AllIn),
                (300, Some(b_hole), PlayerStatus::Active),
                (300, Some(c_hole), PlayerStatus::Active),
            ],
            board,
        );
        let _ = settle(&mut s);
        // A 赢主池 300；B 赢边池 400 (200 from B + 200 from C)
        assert_eq!(s.players[0].stack, 300);
        assert_eq!(s.players[1].stack, 400);
        assert_eq!(s.players[2].stack, 0);
    }

    #[test]
    fn folded_player_no_share() {
        // A,B 都投 100，C fold 但贡献了 50（盲注情境）
        let board = vec![
            c(Rank::Two, Suit::Hearts),
            c(Rank::Three, Suit::Diamonds),
            c(Rank::Four, Suit::Clubs),
            c(Rank::Nine, Suit::Spades),
            c(Rank::Jack, Suit::Hearts),
        ];
        let a_hole = [c(Rank::Ace, Suit::Hearts), c(Rank::Ace, Suit::Spades)];
        let b_hole = [c(Rank::King, Suit::Hearts), c(Rank::King, Suit::Spades)];
        let mut s = mk_state(
            vec![
                (100, Some(a_hole), PlayerStatus::Active),
                (100, Some(b_hole), PlayerStatus::Active),
                (50, None, PlayerStatus::Folded),
            ],
            board,
        );
        let _ = settle(&mut s);
        // 主池：3 * 50 = 150 (eligible A,B) → A 赢
        // 边池：2 * 50 = 100 (eligible A,B) → A 赢
        assert_eq!(s.players[0].stack, 250);
        assert_eq!(s.players[1].stack, 0);
    }
}
