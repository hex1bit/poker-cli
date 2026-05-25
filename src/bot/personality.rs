//! Bot 性格档案与预置 6 种风格。
//!
//! 每个性格通过若干 0..1 / 0..∞ 的参数描述其打法倾向；
//! `decision.rs` 会把这些参数与当前手牌强度、底池赔率、位置等组合成具体决策。

#[derive(Debug, Clone, Copy)]
pub struct Personality {
    /// 自愿入池频率 (Voluntarily Put In Pot)，∈ [0,1]。
    /// 影响 preflop “跟随别人下注” 的容忍阈值。
    pub vpip: f64,
    /// Preflop Raise 频率，∈ [0,1]。决定 preflop 主动加注的倾向。
    pub pfr: f64,
    /// 进入下注轮后 raise/call 的比，越大越激进。1.0 = 中性，2+ 较激进。
    pub aggression: f64,
    /// 弱牌假装强牌（诈唬）的概率 ∈ [0,1]。
    pub bluff_freq: f64,
    /// 强牌假装弱牌（钓鱼）概率 ∈ [0,1]。
    pub slowplay_freq: f64,
    /// flop 持续下注 (cbet) 频率（作为上一街进攻者时）。
    pub cbet_freq: f64,
    /// tilt（情绪）系数 0..1：连输后短期变激进。
    pub tilt_factor: f64,
    /// 位置敏感度 ∈ [0,1]：1=完全按位置调整入池阈值，0=不在乎位置。
    pub position_aware: f64,
    /// 弃牌时把一张底牌秀出来的概率 ∈ [0,1]。
    pub show_freq: f64,
    /// 显示名。
    pub label: &'static str,
}

impl Personality {
    pub const ROCK: Personality = Personality {
        vpip: 0.14,
        pfr: 0.08,
        aggression: 0.8,
        bluff_freq: 0.02,
        slowplay_freq: 0.05,
        cbet_freq: 0.50,
        tilt_factor: 0.10,
        position_aware: 0.30,
        show_freq: 0.02,
        label: "Rock",
    };

    pub const SHARK: Personality = Personality {
        vpip: 0.22,
        pfr: 0.18,
        aggression: 2.5,
        bluff_freq: 0.12,
        slowplay_freq: 0.15,
        cbet_freq: 0.70,
        tilt_factor: 0.10,
        position_aware: 0.80,
        show_freq: 0.05,
        label: "Shark",
    };

    pub const MANIAC: Personality = Personality {
        vpip: 0.42,
        pfr: 0.35,
        aggression: 4.0,
        bluff_freq: 0.30,
        slowplay_freq: 0.05,
        cbet_freq: 0.85,
        tilt_factor: 0.40,
        position_aware: 0.30,
        show_freq: 0.20,
        label: "Maniac",
    };

    pub const FISH: Personality = Personality {
        vpip: 0.55,
        pfr: 0.08,
        aggression: 0.5,
        bluff_freq: 0.03,
        slowplay_freq: 0.10,
        cbet_freq: 0.25,
        tilt_factor: 0.20,
        position_aware: 0.10,
        show_freq: 0.10,
        label: "Fish",
    };

    pub const TRAPPER: Personality = Personality {
        vpip: 0.20,
        pfr: 0.10,
        aggression: 1.5,
        bluff_freq: 0.08,
        slowplay_freq: 0.40,
        cbet_freq: 0.45,
        tilt_factor: 0.10,
        position_aware: 0.60,
        show_freq: 0.08,
        label: "Trapper",
    };

    pub const BLUFFER: Personality = Personality {
        vpip: 0.30,
        pfr: 0.25,
        aggression: 3.0,
        bluff_freq: 0.45,
        slowplay_freq: 0.10,
        cbet_freq: 0.80,
        tilt_factor: 0.30,
        position_aware: 0.50,
        show_freq: 0.25,
        label: "Bluffer",
    };

    /// 预置 6 种顺序列表，按用户偏好顺序展示。
    pub const PRESETS: [Personality; 6] = [
        Personality::ROCK,
        Personality::SHARK,
        Personality::MANIAC,
        Personality::FISH,
        Personality::TRAPPER,
        Personality::BLUFFER,
    ];

    /// 按名字查找（大小写不敏感）。
    pub fn from_label(s: &str) -> Option<Personality> {
        let s = s.to_ascii_lowercase();
        Self::PRESETS
            .iter()
            .find(|p| p.label.to_ascii_lowercase() == s)
            .copied()
    }
}
