//! 5/7 张牌的牌型评估。
//!
//! `HandRank` 自带充足踢脚信息，直接 `Ord` 比较即可决定胜负。
//! 7 张评估走"直接位图 + 计数"快速路径，无堆分配；
//! `evaluate_7_with_best` 仍走 21-组合枚举（仅用于摊牌展示）。

use crate::card::{Card, Rank};

/// 牌型分类。`Ord` 派生：先按 Category，再按内含的踢脚向量比较。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Category {
    HighCard = 0,
    Pair = 1,
    TwoPair = 2,
    ThreeKind = 3,
    Straight = 4,
    Flush = 5,
    FullHouse = 6,
    FourKind = 7,
    StraightFlush = 8,
}

/// 完整牌力描述：(category, 五个比较键)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HandRank {
    pub category: Category,
    pub tiebreak: [u8; 5],
}

impl HandRank {
    fn new(category: Category, tiebreak: [u8; 5]) -> Self {
        Self { category, tiebreak }
    }
}

/// 评估恰好 5 张牌，返回 HandRank。无堆分配。
pub fn evaluate_5(cards: &[Card; 5]) -> HandRank {
    let mut rank_cnt = [0u8; 13];
    let mut suit_cnt = [0u8; 4];
    let mut rank_bits: u16 = 0;
    for c in cards {
        let r = c.rank().as_u8();
        let s = c.suit().as_u8();
        rank_cnt[r as usize] += 1;
        suit_cnt[s as usize] += 1;
        rank_bits |= 1u16 << r;
    }
    let is_flush = suit_cnt.iter().any(|&c| c == 5);
    let straight_top = detect_straight_bits(rank_bits);

    if is_flush && let Some(top) = straight_top {
        return HandRank::new(Category::StraightFlush, [top, 0, 0, 0, 0]);
    }

    let mut quad: Option<u8> = None;
    let mut trips: Option<u8> = None;
    let mut pairs: [u8; 2] = [0; 2];
    let mut pairs_n: usize = 0;
    for r in (0..13u8).rev() {
        match rank_cnt[r as usize] {
            4 => quad = Some(r),
            3 => trips = Some(r),
            2 => {
                if pairs_n < 2 {
                    pairs[pairs_n] = r;
                    pairs_n += 1;
                }
            }
            _ => {}
        }
    }

    if let Some(q) = quad {
        let kicker = highest_rank_excluding(&rank_cnt, &[q]);
        return HandRank::new(Category::FourKind, [q, kicker, 0, 0, 0]);
    }
    if let Some(t) = trips
        && pairs_n >= 1
    {
        return HandRank::new(Category::FullHouse, [t, pairs[0], 0, 0, 0]);
    }
    if is_flush {
        let mut tb = [0u8; 5];
        let mut idx = 0;
        for r in (0..13u8).rev() {
            if rank_bits & (1u16 << r) != 0 {
                tb[idx] = r;
                idx += 1;
                if idx == 5 {
                    break;
                }
            }
        }
        return HandRank::new(Category::Flush, tb);
    }
    if let Some(top) = straight_top {
        return HandRank::new(Category::Straight, [top, 0, 0, 0, 0]);
    }
    if let Some(t) = trips {
        let mut k = [0u8; 2];
        let mut idx = 0;
        for r in (0..13u8).rev() {
            if r != t && rank_cnt[r as usize] > 0 {
                k[idx] = r;
                idx += 1;
                if idx == 2 {
                    break;
                }
            }
        }
        return HandRank::new(Category::ThreeKind, [t, k[0], k[1], 0, 0]);
    }
    if pairs_n == 2 {
        let kicker = highest_rank_excluding(&rank_cnt, &[pairs[0], pairs[1]]);
        return HandRank::new(Category::TwoPair, [pairs[0], pairs[1], kicker, 0, 0]);
    }
    if pairs_n == 1 {
        let p = pairs[0];
        let mut k = [0u8; 3];
        let mut idx = 0;
        for r in (0..13u8).rev() {
            if r != p && rank_cnt[r as usize] > 0 {
                k[idx] = r;
                idx += 1;
                if idx == 3 {
                    break;
                }
            }
        }
        return HandRank::new(Category::Pair, [p, k[0], k[1], k[2], 0]);
    }
    // 高牌
    let mut tb = [0u8; 5];
    let mut idx = 0;
    for r in (0..13u8).rev() {
        if rank_cnt[r as usize] > 0 {
            tb[idx] = r;
            idx += 1;
            if idx == 5 {
                break;
            }
        }
    }
    HandRank::new(Category::HighCard, tb)
}

/// 评估 7 张牌，返回最佳 5 张组成的 HandRank。无堆分配，无 21-组合枚举。
pub fn evaluate_7(cards: &[Card; 7]) -> HandRank {
    let mut rank_cnt = [0u8; 13];
    let mut suit_cnt = [0u8; 4];
    let mut suit_bits = [0u16; 4];
    let mut rank_bits: u16 = 0;
    for c in cards {
        let r = c.rank().as_u8();
        let s = c.suit().as_u8();
        rank_cnt[r as usize] += 1;
        suit_cnt[s as usize] += 1;
        suit_bits[s as usize] |= 1u16 << r;
        rank_bits |= 1u16 << r;
    }

    // 同花花色（最多一种 ≥5）
    let flush_suit = (0..4usize).find(|&s| suit_cnt[s] >= 5);

    // 同花顺
    if let Some(s) = flush_suit
        && let Some(top) = detect_straight_bits(suit_bits[s])
    {
        return HandRank::new(Category::StraightFlush, [top, 0, 0, 0, 0]);
    }

    // 收集 count groups（按 rank 降序）
    let mut quad: Option<u8> = None;
    let mut trips: [u8; 2] = [0; 2];
    let mut trips_n: usize = 0;
    let mut pairs: [u8; 3] = [0; 3];
    let mut pairs_n: usize = 0;
    for r in (0..13u8).rev() {
        match rank_cnt[r as usize] {
            4 => {
                if quad.is_none() {
                    quad = Some(r);
                }
            }
            3 => {
                if trips_n < 2 {
                    trips[trips_n] = r;
                    trips_n += 1;
                }
            }
            2 => {
                if pairs_n < 3 {
                    pairs[pairs_n] = r;
                    pairs_n += 1;
                }
            }
            _ => {}
        }
    }

    // 四条
    if let Some(q) = quad {
        let kicker = highest_rank_excluding(&rank_cnt, &[q]);
        return HandRank::new(Category::FourKind, [q, kicker, 0, 0, 0]);
    }

    // 葫芦：三条 + 对子，或 两个三条（取低三条作对）
    if trips_n >= 1 {
        let t = trips[0];
        if pairs_n >= 1 {
            return HandRank::new(Category::FullHouse, [t, pairs[0], 0, 0, 0]);
        }
        if trips_n >= 2 {
            return HandRank::new(Category::FullHouse, [t, trips[1], 0, 0, 0]);
        }
    }

    // 同花
    if let Some(s) = flush_suit {
        let bits = suit_bits[s];
        let mut tb = [0u8; 5];
        let mut idx = 0;
        for r in (0..13u8).rev() {
            if bits & (1u16 << r) != 0 {
                tb[idx] = r;
                idx += 1;
                if idx == 5 {
                    break;
                }
            }
        }
        return HandRank::new(Category::Flush, tb);
    }

    // 顺子
    if let Some(top) = detect_straight_bits(rank_bits) {
        return HandRank::new(Category::Straight, [top, 0, 0, 0, 0]);
    }

    // 三条
    if trips_n >= 1 {
        let t = trips[0];
        let mut k = [0u8; 2];
        let mut idx = 0;
        for r in (0..13u8).rev() {
            if r != t && rank_cnt[r as usize] > 0 {
                k[idx] = r;
                idx += 1;
                if idx == 2 {
                    break;
                }
            }
        }
        return HandRank::new(Category::ThreeKind, [t, k[0], k[1], 0, 0]);
    }

    // 两对
    if pairs_n >= 2 {
        let hi = pairs[0];
        let lo = pairs[1];
        let kicker = highest_rank_excluding(&rank_cnt, &[hi, lo]);
        return HandRank::new(Category::TwoPair, [hi, lo, kicker, 0, 0]);
    }

    // 一对
    if pairs_n == 1 {
        let p = pairs[0];
        let mut k = [0u8; 3];
        let mut idx = 0;
        for r in (0..13u8).rev() {
            if r != p && rank_cnt[r as usize] > 0 {
                k[idx] = r;
                idx += 1;
                if idx == 3 {
                    break;
                }
            }
        }
        return HandRank::new(Category::Pair, [p, k[0], k[1], k[2], 0]);
    }

    // 高牌
    let mut tb = [0u8; 5];
    let mut idx = 0;
    for r in (0..13u8).rev() {
        if rank_cnt[r as usize] > 0 {
            tb[idx] = r;
            idx += 1;
            if idx == 5 {
                break;
            }
        }
    }
    HandRank::new(Category::HighCard, tb)
}

/// 评估 7 张中最优 5 张组合，并返回该组合（用于摊牌展示，非热路径）。
pub fn evaluate_7_with_best(cards: &[Card; 7]) -> (HandRank, [Card; 5]) {
    let mut best: Option<HandRank> = None;
    let mut best_cards: Option<[Card; 5]> = None;
    for a in 0..3 {
        for b in (a + 1)..4 {
            for c in (b + 1)..5 {
                for d in (c + 1)..6 {
                    for e in (d + 1)..7 {
                        let five = [cards[a], cards[b], cards[c], cards[d], cards[e]];
                        let r = evaluate_5(&five);
                        match best {
                            None => {
                                best = Some(r);
                                best_cards = Some(five);
                            }
                            Some(cur) if r > cur => {
                                best = Some(r);
                                best_cards = Some(five);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    (
        best.expect("21 combos enumerated"),
        best_cards.expect("21 combos enumerated"),
    )
}

/// 给出顺子最高牌的 rank 索引（0..12）。识别 A-2-3-4-5（轮子）。
fn detect_straight_bits(bits: u16) -> Option<u8> {
    // 从高到低扫 5 连
    for top in (4..=12u8).rev() {
        let mask = 0b11111u16 << (top - 4);
        if (bits & mask) == mask {
            return Some(top);
        }
    }
    // 轮子 A2345
    let wheel = (1u16 << Rank::Ace.as_u8())
        | (1u16 << Rank::Two.as_u8())
        | (1u16 << Rank::Three.as_u8())
        | (1u16 << Rank::Four.as_u8())
        | (1u16 << Rank::Five.as_u8());
    if (bits & wheel) == wheel {
        return Some(Rank::Five.as_u8());
    }
    None
}

fn highest_rank_excluding(rank_cnt: &[u8; 13], excl: &[u8]) -> u8 {
    for r in (0..13u8).rev() {
        if rank_cnt[r as usize] > 0 && !excl.contains(&r) {
            return r;
        }
    }
    0
}

/// 返回适合日志展示的牌型描述。
pub fn describe_rank(rank: HandRank) -> String {
    let r = |v: u8| {
        Rank::from_u8(v)
            .expect("rank tiebreak should be valid")
            .label()
    };
    match rank.category {
        Category::HighCard => format!("高牌 {}", r(rank.tiebreak[0])),
        Category::Pair => format!("一对 {}", r(rank.tiebreak[0])),
        Category::TwoPair => format!("两对 {} 和 {}", r(rank.tiebreak[0]), r(rank.tiebreak[1])),
        Category::ThreeKind => format!("三条 {}", r(rank.tiebreak[0])),
        Category::Straight => format!("顺子，到 {}", r(rank.tiebreak[0])),
        Category::Flush => format!("同花，{} 高", r(rank.tiebreak[0])),
        Category::FullHouse => format!("葫芦，{} 带 {}", r(rank.tiebreak[0]), r(rank.tiebreak[1])),
        Category::FourKind => format!("四条 {}", r(rank.tiebreak[0])),
        Category::StraightFlush => format!("同花顺，到 {}", r(rank.tiebreak[0])),
    }
}

/// 评估 7 张牌并返回日志展示文本。
pub fn describe_7(cards: &[Card; 7]) -> String {
    describe_rank(evaluate_7(cards))
}

/// 给真人玩家做的"当前最佳牌型"预览：根据已发的公共牌数量自动选择评估方式。
/// `community.len()` 必须 ∈ {3, 4, 5}，否则返回 None。
pub fn describe_hero(hole: [Card; 2], community: &[Card]) -> Option<String> {
    match community.len() {
        3 => {
            let five = [hole[0], hole[1], community[0], community[1], community[2]];
            Some(describe_rank(evaluate_5(&five)))
        }
        4 => {
            let six = [
                hole[0],
                hole[1],
                community[0],
                community[1],
                community[2],
                community[3],
            ];
            // C(6,5) = 6 组合：每次跳过一张
            let mut best: Option<HandRank> = None;
            for skip in 0..6 {
                let mut five = [six[0]; 5];
                let mut idx = 0;
                for (i, card) in six.iter().enumerate() {
                    if i != skip {
                        five[idx] = *card;
                        idx += 1;
                    }
                }
                let r = evaluate_5(&five);
                best = Some(match best {
                    Some(cur) if cur >= r => cur,
                    _ => r,
                });
            }
            best.map(describe_rank)
        }
        5 => {
            let seven = [
                hole[0],
                hole[1],
                community[0],
                community[1],
                community[2],
                community[3],
                community[4],
            ];
            Some(describe_7(&seven))
        }
        _ => None,
    }
}

/// 评估 7 张牌，返回牌型描述和实际采用的最佳 5 张牌。
pub fn describe_7_with_best(cards: &[Card; 7]) -> (String, [Card; 5]) {
    let (rank, best) = evaluate_7_with_best(cards);
    (describe_rank(rank), best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Card, Rank, Suit};

    fn c(r: Rank, s: Suit) -> Card {
        Card::new(r, s)
    }

    #[test]
    fn detects_royal_flush() {
        let hand = [
            c(Rank::Ten, Suit::Hearts),
            c(Rank::Jack, Suit::Hearts),
            c(Rank::Queen, Suit::Hearts),
            c(Rank::King, Suit::Hearts),
            c(Rank::Ace, Suit::Hearts),
        ];
        let r = evaluate_5(&hand);
        assert_eq!(r.category, Category::StraightFlush);
        assert_eq!(r.tiebreak[0], Rank::Ace.as_u8());
    }

    #[test]
    fn detects_wheel_straight() {
        let hand = [
            c(Rank::Ace, Suit::Hearts),
            c(Rank::Two, Suit::Diamonds),
            c(Rank::Three, Suit::Clubs),
            c(Rank::Four, Suit::Spades),
            c(Rank::Five, Suit::Hearts),
        ];
        let r = evaluate_5(&hand);
        assert_eq!(r.category, Category::Straight);
        assert_eq!(r.tiebreak[0], Rank::Five.as_u8());
    }

    #[test]
    fn detects_wheel_straight_flush() {
        let hand = [
            c(Rank::Ace, Suit::Hearts),
            c(Rank::Two, Suit::Hearts),
            c(Rank::Three, Suit::Hearts),
            c(Rank::Four, Suit::Hearts),
            c(Rank::Five, Suit::Hearts),
        ];
        let r = evaluate_5(&hand);
        assert_eq!(r.category, Category::StraightFlush);
        assert_eq!(r.tiebreak[0], Rank::Five.as_u8());
    }

    #[test]
    fn detects_four_of_a_kind() {
        let hand = [
            c(Rank::Ace, Suit::Hearts),
            c(Rank::Ace, Suit::Diamonds),
            c(Rank::Ace, Suit::Clubs),
            c(Rank::Ace, Suit::Spades),
            c(Rank::King, Suit::Hearts),
        ];
        let r = evaluate_5(&hand);
        assert_eq!(r.category, Category::FourKind);
        assert_eq!(r.tiebreak[0], Rank::Ace.as_u8());
        assert_eq!(r.tiebreak[1], Rank::King.as_u8());
    }

    #[test]
    fn detects_full_house() {
        let hand = [
            c(Rank::Ten, Suit::Hearts),
            c(Rank::Ten, Suit::Diamonds),
            c(Rank::Ten, Suit::Clubs),
            c(Rank::Two, Suit::Spades),
            c(Rank::Two, Suit::Hearts),
        ];
        let r = evaluate_5(&hand);
        assert_eq!(r.category, Category::FullHouse);
        assert_eq!(r.tiebreak[0], Rank::Ten.as_u8());
        assert_eq!(r.tiebreak[1], Rank::Two.as_u8());
    }

    #[test]
    fn detects_two_pair_ordering() {
        let hi = [
            c(Rank::King, Suit::Hearts),
            c(Rank::King, Suit::Diamonds),
            c(Rank::Two, Suit::Clubs),
            c(Rank::Two, Suit::Spades),
            c(Rank::Five, Suit::Hearts),
        ];
        let lo = [
            c(Rank::Queen, Suit::Hearts),
            c(Rank::Queen, Suit::Diamonds),
            c(Rank::Jack, Suit::Clubs),
            c(Rank::Jack, Suit::Spades),
            c(Rank::Five, Suit::Hearts),
        ];
        let rh = evaluate_5(&hi);
        let rl = evaluate_5(&lo);
        assert!(rh > rl);
    }

    #[test]
    fn kicker_decides_pair() {
        let a = [
            c(Rank::Ace, Suit::Hearts),
            c(Rank::Ace, Suit::Diamonds),
            c(Rank::King, Suit::Clubs),
            c(Rank::Five, Suit::Spades),
            c(Rank::Three, Suit::Hearts),
        ];
        let b = [
            c(Rank::Ace, Suit::Hearts),
            c(Rank::Ace, Suit::Diamonds),
            c(Rank::Queen, Suit::Clubs),
            c(Rank::Jack, Suit::Spades),
            c(Rank::Ten, Suit::Hearts),
        ];
        assert!(evaluate_5(&a) > evaluate_5(&b));
    }

    #[test]
    fn flush_beats_straight() {
        let flush = [
            c(Rank::Two, Suit::Hearts),
            c(Rank::Four, Suit::Hearts),
            c(Rank::Six, Suit::Hearts),
            c(Rank::Eight, Suit::Hearts),
            c(Rank::Ten, Suit::Hearts),
        ];
        let straight = [
            c(Rank::Nine, Suit::Hearts),
            c(Rank::Ten, Suit::Diamonds),
            c(Rank::Jack, Suit::Clubs),
            c(Rank::Queen, Suit::Spades),
            c(Rank::King, Suit::Hearts),
        ];
        assert!(evaluate_5(&flush) > evaluate_5(&straight));
    }

    #[test]
    fn category_order() {
        assert!(Category::StraightFlush > Category::FourKind);
        assert!(Category::FourKind > Category::FullHouse);
        assert!(Category::FullHouse > Category::Flush);
        assert!(Category::Flush > Category::Straight);
        assert!(Category::Straight > Category::ThreeKind);
        assert!(Category::ThreeKind > Category::TwoPair);
        assert!(Category::TwoPair > Category::Pair);
        assert!(Category::Pair > Category::HighCard);
    }

    #[test]
    fn evaluate_7_picks_best() {
        let hand = [
            c(Rank::Ten, Suit::Hearts),
            c(Rank::Jack, Suit::Hearts),
            c(Rank::Queen, Suit::Hearts),
            c(Rank::King, Suit::Hearts),
            c(Rank::Ace, Suit::Hearts),
            c(Rank::Two, Suit::Clubs),
            c(Rank::Two, Suit::Spades),
        ];
        let r = evaluate_7(&hand);
        assert_eq!(r.category, Category::StraightFlush);
        assert_eq!(r.tiebreak[0], Rank::Ace.as_u8());
    }

    #[test]
    fn evaluate_7_two_pair_vs_set() {
        let hand = [
            c(Rank::Ace, Suit::Hearts),
            c(Rank::Ace, Suit::Diamonds),
            c(Rank::Ace, Suit::Clubs),
            c(Rank::Five, Suit::Spades),
            c(Rank::Five, Suit::Hearts),
            c(Rank::Nine, Suit::Diamonds),
            c(Rank::King, Suit::Clubs),
        ];
        let r = evaluate_7(&hand);
        assert_eq!(r.category, Category::FullHouse);
    }

    /// 与 21-组合枚举 (慢路径) 在大量随机 7 张牌组合上保持一致。
    #[test]
    fn evaluate_7_matches_brute_force() {
        use rand::SeedableRng;
        use rand::seq::SliceRandom;
        let mut rng = rand::rngs::StdRng::seed_from_u64(0xC0DEFEED);
        let deck: Vec<Card> = (0..52u8).map(|i| Card::from_index(i).unwrap()).collect();
        for _ in 0..2000 {
            let mut d = deck.clone();
            d.shuffle(&mut rng);
            let cards: [Card; 7] = [d[0], d[1], d[2], d[3], d[4], d[5], d[6]];
            let fast = evaluate_7(&cards);
            let slow = evaluate_7_with_best(&cards).0;
            assert_eq!(
                fast, slow,
                "fast vs brute-force diverged on {:?}\n fast={:?}\n slow={:?}",
                cards, fast, slow
            );
        }
    }

    #[test]
    fn evaluate_7_two_trips_full_house() {
        // 两组 trips：AAA + KKK + 散
        let hand = [
            c(Rank::Ace, Suit::Hearts),
            c(Rank::Ace, Suit::Diamonds),
            c(Rank::Ace, Suit::Clubs),
            c(Rank::King, Suit::Spades),
            c(Rank::King, Suit::Hearts),
            c(Rank::King, Suit::Diamonds),
            c(Rank::Two, Suit::Clubs),
        ];
        let r = evaluate_7(&hand);
        assert_eq!(r.category, Category::FullHouse);
        assert_eq!(r.tiebreak[0], Rank::Ace.as_u8());
        assert_eq!(r.tiebreak[1], Rank::King.as_u8());
    }
}
