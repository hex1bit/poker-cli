//! 5/7 张牌的牌型评估。
//!
//! `HandRank` 自带充足踢脚信息，直接 `Ord` 比较即可决定胜负。
//! 7 张评估通过枚举 C(7,5)=21 种组合并取最大实现。

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
///
/// 用统一的 5 元素 tiebreaker 数组：
/// - HighCard: 5 张降序 rank
/// - Pair: [pair_rank, k1, k2, k3, 0]
/// - TwoPair: [hi_pair, lo_pair, kicker, 0, 0]
/// - ThreeKind: [trips, k1, k2, 0, 0]
/// - Straight / StraightFlush: [top_card, 0, 0, 0, 0]
/// - Flush: 5 张降序 rank
/// - FullHouse: [trips, pair, 0, 0, 0]
/// - FourKind: [quads, kicker, 0, 0, 0]
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

/// 评估恰好 5 张牌，返回 HandRank。
pub fn evaluate_5(cards: &[Card; 5]) -> HandRank {
    // rank 计数（0..13），suit 计数（0..4）
    let mut rank_cnt = [0u8; 13];
    let mut suit_cnt = [0u8; 4];
    let mut ranks_sorted: [u8; 5] = [0; 5];
    for (i, c) in cards.iter().enumerate() {
        rank_cnt[c.rank().as_u8() as usize] += 1;
        suit_cnt[c.suit().as_u8() as usize] += 1;
        ranks_sorted[i] = c.rank().as_u8();
    }
    ranks_sorted.sort_unstable_by(|a, b| b.cmp(a)); // 降序

    let is_flush = suit_cnt.iter().any(|&n| n == 5);

    // 检测顺子：返回顺子最高牌（Ace-low straight 即 A2345 返回 5=Rank::Five.as_u8()=3）
    let straight_top: Option<u8> = detect_straight(&rank_cnt);

    if is_flush && straight_top.is_some() {
        return HandRank::new(Category::StraightFlush, [straight_top.unwrap(), 0, 0, 0, 0]);
    }

    // 收集 (count, rank) 对，先按 count 降序，再按 rank 降序
    let mut groups: Vec<(u8, u8)> = (0..13u8)
        .filter(|&r| rank_cnt[r as usize] > 0)
        .map(|r| (rank_cnt[r as usize], r))
        .collect();
    groups.sort_unstable_by(|a, b| b.cmp(a));

    match groups[0].0 {
        4 => {
            // 四条
            let quads = groups[0].1;
            let kicker = groups[1].1;
            HandRank::new(Category::FourKind, [quads, kicker, 0, 0, 0])
        }
        3 if groups.len() > 1 && groups[1].0 >= 2 => {
            // 葫芦
            HandRank::new(Category::FullHouse, [groups[0].1, groups[1].1, 0, 0, 0])
        }
        _ if is_flush => {
            let mut t = [0u8; 5];
            t.copy_from_slice(&ranks_sorted);
            HandRank::new(Category::Flush, t)
        }
        _ if straight_top.is_some() => {
            HandRank::new(Category::Straight, [straight_top.unwrap(), 0, 0, 0, 0])
        }
        3 => {
            // 三条
            let trips = groups[0].1;
            let k1 = groups[1].1;
            let k2 = if groups.len() > 2 { groups[2].1 } else { 0 };
            HandRank::new(Category::ThreeKind, [trips, k1, k2, 0, 0])
        }
        2 if groups.len() > 1 && groups[1].0 == 2 => {
            // 两对
            let hi = groups[0].1.max(groups[1].1);
            let lo = groups[0].1.min(groups[1].1);
            let kicker = groups[2].1;
            HandRank::new(Category::TwoPair, [hi, lo, kicker, 0, 0])
        }
        2 => {
            // 一对
            let pair = groups[0].1;
            let k1 = groups[1].1;
            let k2 = groups[2].1;
            let k3 = groups[3].1;
            HandRank::new(Category::Pair, [pair, k1, k2, k3, 0])
        }
        _ => {
            // 高牌
            let mut t = [0u8; 5];
            t.copy_from_slice(&ranks_sorted);
            HandRank::new(Category::HighCard, t)
        }
    }
}

/// 返回顺子的最高牌 rank（0..12）。考虑 A-2-3-4-5 (轮子)，最高牌为 5 (Rank::Five=3)。
fn detect_straight(rank_cnt: &[u8; 13]) -> Option<u8> {
    // bitmap: bit i set ⇔ rank i 至少出现一次
    let mut bits: u16 = 0;
    for i in 0..13 {
        if rank_cnt[i] > 0 {
            bits |= 1 << i;
        }
    }
    // Ace 作 -1 处理：检查 A2345（bits 含 Two..Five 与 Ace）
    let ace_bit = 1u16 << (Rank::Ace.as_u8());
    let low_straight = (1u16 << Rank::Two.as_u8())
        | (1u16 << Rank::Three.as_u8())
        | (1u16 << Rank::Four.as_u8())
        | (1u16 << Rank::Five.as_u8());
    if (bits & ace_bit) != 0 && (bits & low_straight) == low_straight {
        return Some(Rank::Five.as_u8());
    }
    // 一般顺子：从高到低找连续 5 个 bit。
    for top in (4..=12i32).rev() {
        let mask = 0b11111u16 << (top - 4);
        if (bits & mask) == mask {
            return Some(top as u8);
        }
    }
    None
}

/// 评估 7 张中最优 5 张组合。
pub fn evaluate_7(cards: &[Card; 7]) -> HandRank {
    let mut best: Option<HandRank> = None;
    // C(7,5)=21 个组合
    for a in 0..3 {
        for b in (a + 1)..4 {
            for c in (b + 1)..5 {
                for d in (c + 1)..6 {
                    for e in (d + 1)..7 {
                        let five = [cards[a], cards[b], cards[c], cards[d], cards[e]];
                        let r = evaluate_5(&five);
                        match best {
                            None => best = Some(r),
                            Some(cur) if r > cur => best = Some(r),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    best.expect("21 combos enumerated")
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
        // A-2-3-4-5
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
        // KKxx 22 5 > QQ JJ 5? K=11, Q=10 → hi > lo
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
        // Both AA. A's kicker K > Q ⇒ a wins.
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
        // 7 张里包含一个 royal flush + 一个 noise pair, 应识别 royal flush
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
        // hole: AA, board: A 5 5 9 K  -> full house A's full of 5s
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
}
