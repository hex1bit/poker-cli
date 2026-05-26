//! Bot 打牌能力档位。
//!
//! Personality 描述风格，SkillLevel 描述水平。水平越高，equity 估算越稳定、
//! 对对手画像的利用越强，随机误判越少。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillLevel {
    Rookie,
    Regular,
    Pro,
}

impl SkillLevel {
    pub const DEFAULT: SkillLevel = SkillLevel::Regular;

    pub fn from_label(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "rookie" | "easy" | "beginner" | "菜鸟" => Some(Self::Rookie),
            "regular" | "normal" | "medium" | "普通" => Some(Self::Regular),
            "pro" | "hard" | "expert" | "高手" => Some(Self::Pro),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Rookie => "Rookie",
            Self::Regular => "Regular",
            Self::Pro => "Pro",
        }
    }

    pub fn mc_iters(self) -> usize {
        match self {
            Self::Rookie => 250,
            Self::Regular => 600,
            Self::Pro => 1_000,
        }
    }

    pub fn equity_noise(self) -> f64 {
        match self {
            Self::Rookie => 0.08,
            Self::Regular => 0.025,
            Self::Pro => 0.0,
        }
    }

    pub fn profile_weight(self) -> f64 {
        match self {
            Self::Rookie => 0.25,
            Self::Regular => 1.0,
            Self::Pro => 1.35,
        }
    }

    pub fn mistake_rate(self) -> f64 {
        match self {
            Self::Rookie => 0.08,
            Self::Regular => 0.025,
            Self::Pro => 0.005,
        }
    }
}

impl std::fmt::Display for SkillLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aliases() {
        assert_eq!(SkillLevel::from_label("easy"), Some(SkillLevel::Rookie));
        assert_eq!(SkillLevel::from_label("normal"), Some(SkillLevel::Regular));
        assert_eq!(SkillLevel::from_label("hard"), Some(SkillLevel::Pro));
    }
}
