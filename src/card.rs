//! 扑克牌基本类型：Rank / Suit / Card / Deck。
//!
//! - `Card` 以紧凑的 `u8 (0..52)` 编码：`rank * 4 + suit_index`。
//! - 比较时只看 `Rank`（按大小），花色仅用于显示与同花判断。

use rand::Rng;
use rand::seq::SliceRandom;
use std::fmt;

/// 牌面大小，2 最小，A 最大。值域 0..13。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Rank {
    Two = 0,
    Three = 1,
    Four = 2,
    Five = 3,
    Six = 4,
    Seven = 5,
    Eight = 6,
    Nine = 7,
    Ten = 8,
    Jack = 9,
    Queen = 10,
    King = 11,
    Ace = 12,
}

impl Rank {
    pub const ALL: [Rank; 13] = [
        Rank::Two,
        Rank::Three,
        Rank::Four,
        Rank::Five,
        Rank::Six,
        Rank::Seven,
        Rank::Eight,
        Rank::Nine,
        Rank::Ten,
        Rank::Jack,
        Rank::Queen,
        Rank::King,
        Rank::Ace,
    ];

    pub fn from_u8(v: u8) -> Option<Rank> {
        if v < 13 {
            Some(Self::ALL[v as usize])
        } else {
            None
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn label(self) -> &'static str {
        match self {
            Rank::Two => "2",
            Rank::Three => "3",
            Rank::Four => "4",
            Rank::Five => "5",
            Rank::Six => "6",
            Rank::Seven => "7",
            Rank::Eight => "8",
            Rank::Nine => "9",
            Rank::Ten => "T",
            Rank::Jack => "J",
            Rank::Queen => "Q",
            Rank::King => "K",
            Rank::Ace => "A",
        }
    }
}

impl fmt::Display for Rank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// 花色。比较意义仅在同花判断中体现。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Suit {
    Clubs = 0,
    Diamonds = 1,
    Hearts = 2,
    Spades = 3,
}

impl Suit {
    pub const ALL: [Suit; 4] = [Suit::Clubs, Suit::Diamonds, Suit::Hearts, Suit::Spades];

    pub fn from_u8(v: u8) -> Option<Suit> {
        if v < 4 {
            Some(Self::ALL[v as usize])
        } else {
            None
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Unicode 花色符号。
    pub fn glyph(self) -> char {
        match self {
            Suit::Clubs => '♣',
            Suit::Diamonds => '♦',
            Suit::Hearts => '♥',
            Suit::Spades => '♠',
        }
    }

    /// 是否为红色花色（用于终端着色）。
    pub fn is_red(self) -> bool {
        matches!(self, Suit::Diamonds | Suit::Hearts)
    }
}

impl fmt::Display for Suit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.glyph())
    }
}

/// 一张扑克牌。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Card(u8);

impl Card {
    /// 由 (rank, suit) 构造。
    pub fn new(rank: Rank, suit: Suit) -> Card {
        Card(rank.as_u8() * 4 + suit.as_u8())
    }

    /// 从 0..52 索引构造。
    pub fn from_index(idx: u8) -> Option<Card> {
        if idx < 52 { Some(Card(idx)) } else { None }
    }

    pub fn index(self) -> u8 {
        self.0
    }

    pub fn rank(self) -> Rank {
        Rank::from_u8(self.0 / 4).expect("valid rank")
    }

    pub fn suit(self) -> Suit {
        Suit::from_u8(self.0 % 4).expect("valid suit")
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.rank(), self.suit())
    }
}

/// 一副完整 52 张牌（顺序固定）。
pub fn full_deck() -> Vec<Card> {
    (0..52).map(Card).collect()
}

/// 洗好的一副牌。
pub fn shuffled_deck<R: Rng + ?Sized>(rng: &mut R) -> Vec<Card> {
    let mut d = full_deck();
    d.shuffle(rng);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deck_has_52_unique_cards() {
        let d = full_deck();
        assert_eq!(d.len(), 52);
        let mut seen = std::collections::HashSet::new();
        for c in d {
            assert!(seen.insert(c.index()), "duplicate: {c}");
        }
    }

    #[test]
    fn card_roundtrip() {
        for i in 0..52u8 {
            let c = Card::from_index(i).unwrap();
            assert_eq!(c.index(), i);
            let r = c.rank();
            let s = c.suit();
            assert_eq!(Card::new(r, s).index(), i);
        }
    }

    #[test]
    fn rank_ordering() {
        assert!(Rank::Ace > Rank::King);
        assert!(Rank::Two < Rank::Three);
    }

    #[test]
    fn display_card() {
        let c = Card::new(Rank::Ace, Suit::Spades);
        let s = format!("{c}");
        assert!(s.contains('A'));
        assert!(s.contains('♠'));
    }

    #[test]
    fn shuffle_preserves_set() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let d = shuffled_deck(&mut rng);
        assert_eq!(d.len(), 52);
        let set: std::collections::HashSet<_> = d.iter().map(|c| c.index()).collect();
        assert_eq!(set.len(), 52);
    }
}
