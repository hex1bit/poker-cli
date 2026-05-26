//! 胜率 (equity) 估算：蒙特卡洛 + preflop 启发式打分。
//!
//! - `mc_equity`：给定 hero 手牌 + 已知公共牌 + 对手数，随机抽样剩余对手手牌与
//!   未知公共牌，跑 N 次摊牌，返回胜率 (胜 + 平/2 折算)。
//! - `preflop_strength`：169 类型 hand 的简单启发式分数 ∈ [0,1]，
//!   用于 preflop 快速判断 (无需 MC)。这是个粗略的桌面分类，不是严谨 equity。

use rand::Rng;

use crate::card::{Card, Rank};
use crate::hand_eval::{HandRank, evaluate_7};

/// 跑 `iters` 次蒙特卡洛抽样，返回 hero 对 `opponents` 个未知对手的胜率 ∈ [0,1]。
///
/// `community` 长度可以是 0/3/4/5。Panics 如果不合法（>5）或 hero 不是 2 张。
pub fn mc_equity<R: Rng + ?Sized>(
    hero: [Card; 2],
    community: &[Card],
    opponents: usize,
    iters: usize,
    rng: &mut R,
) -> f64 {
    assert!(community.len() <= 5, "community size must be ≤ 5");
    assert!(opponents >= 1, "need at least 1 opponent");

    // 构造剩余牌池：u64 位图 + 栈数组，零堆分配
    let mut used: u64 = 0;
    used |= 1u64 << hero[0].index();
    used |= 1u64 << hero[1].index();
    for c in community {
        used |= 1u64 << c.index();
    }

    let placeholder = Card::from_index(0).unwrap();
    let mut pool_buf = [placeholder; 52];
    let mut pool_len: usize = 0;
    for i in 0..52u8 {
        if used & (1u64 << i) == 0 {
            pool_buf[pool_len] = Card::from_index(i).unwrap();
            pool_len += 1;
        }
    }
    let pool = &mut pool_buf[..pool_len];

    let need_community = 5 - community.len();
    let need_opp = opponents * 2;
    let need_total = need_community + need_opp;
    assert!(pool.len() >= need_total, "not enough cards for simulation");

    // 公共牌前缀拷到 board，后续每次只填后缀
    let mut board = [placeholder; 5];
    for (i, c) in community.iter().enumerate() {
        board[i] = *c;
    }

    // River：hero 牌力固定，外提
    let hero_rank_fixed = if need_community == 0 {
        Some(evaluate_7(&[
            hero[0], hero[1], board[0], board[1], board[2], board[3], board[4],
        ]))
    } else {
        None
    };

    let mut wins = 0.0f64;

    for _ in 0..iters {
        // Fisher-Yates 部分洗：只洗前 need_total 张
        partial_shuffle(pool, need_total, rng);

        for i in 0..need_community {
            board[community.len() + i] = pool[i];
        }

        let hero_rank = match hero_rank_fixed {
            Some(r) => r,
            None => evaluate_7(&[
                hero[0], hero[1], board[0], board[1], board[2], board[3], board[4],
            ]),
        };

        let mut best_opp: Option<HandRank> = None;
        let mut tied_with_best = 0u32;
        for o in 0..opponents {
            let i = need_community + o * 2;
            let r = evaluate_7(&[
                pool[i],
                pool[i + 1],
                board[0],
                board[1],
                board[2],
                board[3],
                board[4],
            ]);
            match best_opp {
                None => {
                    best_opp = Some(r);
                    tied_with_best = 1;
                }
                Some(cur) => {
                    if r > cur {
                        best_opp = Some(r);
                        tied_with_best = 1;
                    } else if r == cur {
                        tied_with_best += 1;
                    }
                }
            }
        }
        let best_opp = best_opp.unwrap();
        if hero_rank > best_opp {
            wins += 1.0;
        } else if hero_rank == best_opp {
            wins += 1.0 / (tied_with_best as f64 + 1.0);
        }
    }

    wins / iters as f64
}

/// 部分 Fisher-Yates：把 `arr` 的前 `k` 个位置换成随机抽出的 k 张。
fn partial_shuffle<R: Rng + ?Sized, T: Copy>(arr: &mut [T], k: usize, rng: &mut R) {
    let n = arr.len();
    for i in 0..k {
        let j = rng.gen_range(i..n);
        arr.swap(i, j);
    }
}

/// 169 类型起手牌的粗略强度 ∈ [0,1]。仅作为 preflop 快速分类。
///
/// 设计：
/// - 基础分 = 较高那张牌的相对位置 (rank/12) * 0.5
/// - 配对加成：对子 → 高分；高对接近 1.0
/// - 同花加成：+0.07
/// - 连张加成：rank gap 越小越好
/// - 高牌阶梯：A/K/Q/J 显著加成
pub fn preflop_strength(hero: [Card; 2]) -> f64 {
    let (high, low) = if hero[0].rank() >= hero[1].rank() {
        (hero[0].rank(), hero[1].rank())
    } else {
        (hero[1].rank(), hero[0].rank())
    };
    let suited = hero[0].suit() == hero[1].suit();
    let pair = hero[0].rank() == hero[1].rank();
    let h = high.as_u8() as f64; // 0..12
    let l = low.as_u8() as f64;

    if pair {
        // pair: AA=1.0, 22 ≈ 0.50
        let s = 0.50 + (h / 12.0) * 0.50;
        return s.clamp(0.0, 1.0);
    }

    let mut score = 0.20 + (h / 12.0) * 0.35; // 高牌权重
    score += (l / 12.0) * 0.10; // 第二张高度
    if suited {
        score += 0.08;
    }
    // gap 惩罚 / 连张奖励
    let gap = (h - l) as i32;
    let connector_bonus = match gap {
        1 => 0.07,
        2 => 0.04,
        3 => 0.02,
        4 => 0.01,
        _ => 0.0,
    };
    score += connector_bonus;

    // 高牌额外加分（含 A 的手）
    if high == Rank::Ace {
        score += 0.06;
    } else if high == Rank::King && low >= Rank::Ten {
        score += 0.03;
    }

    score.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Card, Rank, Suit};
    use rand::SeedableRng;

    fn c(r: Rank, s: Suit) -> Card {
        Card::new(r, s)
    }

    #[test]
    fn aa_vs_kk_heads_up_known_ratio() {
        // 经典：AA vs KK ≈ 82% (准确 81.9%)
        let hero = [c(Rank::Ace, Suit::Hearts), c(Rank::Ace, Suit::Spades)];
        // 限定对手为 KK：用空 community 跑 simulation 但限定对手 hand 的方法需要特殊接口。
        // 简化：直接做 ad-hoc 全枚举式 MC：替换对手成 KK 后 board 全部抽样。
        // 这里改用专用枚举函数：跑 5000 次 board 抽样 + 固定对手 KK。
        let opp = [c(Rank::King, Suit::Hearts), c(Rank::King, Suit::Spades)];
        let win_rate = fixed_opp_equity(hero, opp, 8000);
        assert!(
            (win_rate - 0.819).abs() < 0.025,
            "AA vs KK win_rate = {win_rate}"
        );
    }

    #[test]
    fn aks_vs_22_close_to_coin_flip() {
        // AKs vs 22 ≈ 50% (实际 AKs 略输 22 ~46-47%, AK off ~48% 等)
        let hero = [c(Rank::Ace, Suit::Hearts), c(Rank::King, Suit::Hearts)];
        let opp = [c(Rank::Two, Suit::Diamonds), c(Rank::Two, Suit::Clubs)];
        let win_rate = fixed_opp_equity(hero, opp, 8000);
        assert!(
            (win_rate - 0.47).abs() < 0.05,
            "AKs vs 22 win_rate = {win_rate}"
        );
    }

    /// 辅助：hero 固定对手手牌的 equity 计算（精确 MC）。
    fn fixed_opp_equity(hero: [Card; 2], opp: [Card; 2], iters: usize) -> f64 {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let mut used = [false; 52];
        for c in [hero[0], hero[1], opp[0], opp[1]] {
            used[c.index() as usize] = true;
        }
        let pool: Vec<Card> = (0..52u8)
            .filter(|&i| !used[i as usize])
            .map(|i| Card::from_index(i).unwrap())
            .collect();
        let mut scratch = pool.clone();
        let mut wins = 0.0;
        for _ in 0..iters {
            partial_shuffle(&mut scratch, 5, &mut rng);
            let board = [scratch[0], scratch[1], scratch[2], scratch[3], scratch[4]];
            let h = evaluate_7(&[
                hero[0], hero[1], board[0], board[1], board[2], board[3], board[4],
            ]);
            let o = evaluate_7(&[
                opp[0], opp[1], board[0], board[1], board[2], board[3], board[4],
            ]);
            if h > o {
                wins += 1.0;
            } else if h == o {
                wins += 0.5;
            }
        }
        wins / iters as f64
    }

    #[test]
    fn mc_equity_sanity_aa_vs_one() {
        // AA vs 1 个随机对手 ≈ 85%
        let mut rng = rand::rngs::StdRng::seed_from_u64(11);
        let hero = [c(Rank::Ace, Suit::Hearts), c(Rank::Ace, Suit::Spades)];
        let eq = mc_equity(hero, &[], 1, 3000, &mut rng);
        assert!(eq > 0.80 && eq < 0.90, "AA vs 1 random equity = {eq}");
    }

    #[test]
    fn mc_equity_more_opponents_lower() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(13);
        let hero = [c(Rank::Ace, Suit::Hearts), c(Rank::Ace, Suit::Spades)];
        let e1 = mc_equity(hero, &[], 1, 2000, &mut rng);
        let e5 = mc_equity(hero, &[], 5, 2000, &mut rng);
        assert!(
            e1 > e5,
            "more opponents should reduce equity, got e1={e1} e5={e5}"
        );
    }

    #[test]
    fn preflop_strength_order() {
        let aa = preflop_strength([c(Rank::Ace, Suit::Hearts), c(Rank::Ace, Suit::Spades)]);
        let twos = preflop_strength([c(Rank::Two, Suit::Hearts), c(Rank::Two, Suit::Spades)]);
        let trash = preflop_strength([c(Rank::Seven, Suit::Hearts), c(Rank::Two, Suit::Spades)]);
        let aks = preflop_strength([c(Rank::Ace, Suit::Hearts), c(Rank::King, Suit::Hearts)]);
        assert!(aa > aks);
        assert!(aks > twos || twos > trash);
        assert!(twos > trash);
        assert!(aa > 0.95);
        assert!(trash < 0.45);
    }
}
