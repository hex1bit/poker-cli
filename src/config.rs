//! CLI 参数与游戏配置。

use clap::Parser;
use rand::Rng;
use rand::seq::SliceRandom;

use crate::bot::{Personality, SkillLevel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TablePreset {
    Default,
    Soft,
    Wild,
    Tough,
    Mixed,
}

impl TablePreset {
    pub fn from_label(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "default" => Some(Self::Default),
            "soft" => Some(Self::Soft),
            "wild" => Some(Self::Wild),
            "tough" => Some(Self::Tough),
            "mixed" => Some(Self::Mixed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Table,
    Square,
    Ratatui,
}

impl Layout {
    pub fn from_label(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "table" | "list" => Some(Self::Table),
            "rectangle" | "rect" | "square" | "box" => Some(Self::Square),
            "ratatui" | "tui" | "widgets" => Some(Self::Ratatui),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotBustPolicy {
    SitOut,
    Rebuy,
    Replace,
}

impl BotBustPolicy {
    pub fn from_label(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "sitout" | "sit-out" | "out" => Some(Self::SitOut),
            "rebuy" | "reload" => Some(Self::Rebuy),
            "replace" | "new" => Some(Self::Replace),
            _ => None,
        }
    }
}

#[derive(Parser, Debug)]
#[command(name = "poker", version, about = "Texas Hold'em CLI vs AI bots")]
pub struct Cli {
    /// AI Bot 数量 (1..=9)。
    #[arg(long, default_value_t = 5)]
    pub bots: usize,
    /// 显式指定 Bot 性格列表，逗号分隔；如 "Rock,Shark,Maniac"。
    #[arg(long)]
    pub personalities: Option<String>,
    /// 未指定 personalities 时，随机分配 Bot 个性。
    #[arg(long, default_value_t = false)]
    pub random_personalities: bool,
    /// 显示玩家 HUD 统计（VPIP / PFR / AF）。
    #[arg(long, default_value_t = false)]
    pub hud: bool,
    /// 牌桌布局：table / rectangle / ratatui；square/box 为 rectangle 别名，tui/widgets 为 ratatui 别名。
    #[arg(long, default_value = "table")]
    pub layout: String,
    /// 桌型预设：default / soft / wild / tough / mixed。
    #[arg(long, default_value = "default")]
    pub table: String,
    /// 每位玩家初始筹码。
    #[arg(long, default_value_t = 1500)]
    pub stack: u64,
    /// 小盲。
    #[arg(long, default_value_t = 5)]
    pub sb: u64,
    /// 大盲。
    #[arg(long, default_value_t = 10)]
    pub bb: u64,
    /// 限制手数；0 = 无限。
    #[arg(long, default_value_t = 0)]
    pub hands: u32,
    /// 真人玩家显示名。
    #[arg(long, default_value = "YOU")]
    pub name: String,
    /// 不输出 bot 台词。
    #[arg(long, default_value_t = false)]
    pub quiet: bool,
    /// 关闭发牌 / 摊牌动画。
    #[arg(long, default_value_t = false)]
    pub no_anim: bool,
    /// Bot 决策前的思考停顿毫秒数；0 = 关闭，--no-anim 也会关闭。
    #[arg(long, default_value_t = 1000)]
    pub bot_think_ms: u64,
    /// Bot 破产后的处理：sitout / rebuy / replace。
    #[arg(long, default_value = "sitout")]
    pub bot_bust_policy: String,
    /// 保留多少手历史用于回放，默认 10。
    #[arg(long, default_value_t = 10)]
    pub replay_keep: usize,
    /// 随机种子；提供后可复现同一局牌。
    #[arg(long)]
    pub seed: Option<u64>,
    /// 将每手牌历史追加写入 JSONL 文件。
    #[arg(long)]
    pub history: Option<String>,
    /// 全局 Bot 打牌能力：rookie / regular / pro。
    #[arg(long, default_value = "regular")]
    pub skill: String,
    /// 显式指定每个 Bot 的能力列表，逗号分隔；如 "rookie,regular,pro"。
    #[arg(long)]
    pub skills: Option<String>,
}

impl Cli {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=9).contains(&self.bots) {
            return Err("bots must be 1..=9".to_string());
        }
        if self.sb == 0 || self.bb == 0 {
            return Err("sb and bb must be positive".to_string());
        }
        if self.sb >= self.bb {
            return Err("sb must be smaller than bb".to_string());
        }
        if self.stack < self.bb {
            return Err("stack must be at least bb".to_string());
        }
        if Layout::from_label(&self.layout).is_none() {
            return Err(format!("unknown layout: {}", self.layout));
        }
        if TablePreset::from_label(&self.table).is_none() {
            return Err(format!("unknown table preset: {}", self.table));
        }
        if BotBustPolicy::from_label(&self.bot_bust_policy).is_none() {
            return Err(format!("unknown bot bust policy: {}", self.bot_bust_policy));
        }
        if let Some(s) = &self.personalities {
            let unknown: Vec<&str> = s
                .split(',')
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .filter(|p| Personality::from_label(p).is_none())
                .collect();
            if !unknown.is_empty() {
                return Err(format!("unknown personalities: {}", unknown.join(", ")));
            }
        }
        if SkillLevel::from_label(&self.skill).is_none() {
            return Err(format!("unknown skill: {}", self.skill));
        }
        if let Some(s) = &self.skills {
            let unknown: Vec<&str> = s
                .split(',')
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .filter(|p| SkillLevel::from_label(p).is_none())
                .collect();
            if !unknown.is_empty() {
                return Err(format!("unknown skills: {}", unknown.join(", ")));
            }
        }
        Ok(())
    }

    /// 选择 bot 性格列表（顺序与座位号 1..=N 对应）。
    pub fn resolve_personalities(&self) -> Vec<Personality> {
        if let Some(s) = &self.personalities {
            let chosen: Vec<Personality> = s
                .split(',')
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .filter_map(Personality::from_label)
                .collect();
            if !chosen.is_empty() {
                return (0..self.bots).map(|i| chosen[i % chosen.len()]).collect();
            }
        }
        Personality::PRESETS
            .iter()
            .copied()
            .take(self.bots.clamp(1, 9))
            .collect()
    }

    pub fn resolve_table_personalities(&self) -> Vec<Personality> {
        let preset = TablePreset::from_label(&self.table).unwrap_or(TablePreset::Default);
        let pool: Vec<Personality> = match preset {
            TablePreset::Default => Personality::PRESETS.to_vec(),
            TablePreset::Soft => vec![
                Personality::FISH,
                Personality::CALLING_STATION,
                Personality::WEAK_TIGHT,
                Personality::GAMBLER,
                Personality::ROCK,
            ],
            TablePreset::Wild => vec![
                Personality::MANIAC,
                Personality::GAMBLER,
                Personality::BLUFFER,
                Personality::CALLING_STATION,
                Personality::SHORT_STACKER,
            ],
            TablePreset::Tough => vec![
                Personality::PRO,
                Personality::SHARK,
                Personality::BALANCED_REG,
                Personality::NIT,
                Personality::SHORT_STACKER,
            ],
            TablePreset::Mixed => Personality::PRESETS.to_vec(),
        };
        (0..self.bots).map(|i| pool[i % pool.len()]).collect()
    }

    /// 使用外部 RNG 选择 bot 性格；用于支持可复现的随机个性桌。
    pub fn resolve_personalities_with_rng<R: Rng + ?Sized>(&self, rng: &mut R) -> Vec<Personality> {
        if self.personalities.is_some() || !self.random_personalities {
            if self.personalities.is_none() && self.table != "default" {
                return self.resolve_table_personalities();
            }
            return self.resolve_personalities();
        }
        let mut presets = self.resolve_table_personalities();
        presets.shuffle(rng);
        (0..self.bots).map(|i| presets[i % presets.len()]).collect()
    }

    /// 选择 bot 能力列表（顺序与座位号 1..=N 对应）。
    pub fn resolve_skills(&self) -> Vec<SkillLevel> {
        if let Some(s) = &self.skills {
            let chosen: Vec<SkillLevel> = s
                .split(',')
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .filter_map(SkillLevel::from_label)
                .collect();
            if !chosen.is_empty() {
                return (0..self.bots).map(|i| chosen[i % chosen.len()]).collect();
            }
        }
        if self.skills.is_none() && self.skill == "regular" {
            let preset = TablePreset::from_label(&self.table).unwrap_or(TablePreset::Default);
            if preset == TablePreset::Soft {
                return (0..self.bots)
                    .map(|i| {
                        if i % 3 == 0 {
                            SkillLevel::Rookie
                        } else {
                            SkillLevel::Regular
                        }
                    })
                    .collect();
            }
            if preset == TablePreset::Tough {
                return (0..self.bots)
                    .map(|i| {
                        if i % 3 == 0 {
                            SkillLevel::Regular
                        } else {
                            SkillLevel::Pro
                        }
                    })
                    .collect();
            }
        }
        let skill = SkillLevel::from_label(&self.skill).unwrap_or(SkillLevel::DEFAULT);
        vec![skill; self.bots]
    }

    pub fn layout(&self) -> Layout {
        Layout::from_label(&self.layout).unwrap_or(Layout::Table)
    }

    pub fn bot_bust_policy(&self) -> BotBustPolicy {
        BotBustPolicy::from_label(&self.bot_bust_policy).unwrap_or(BotBustPolicy::SitOut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_skills_by_repeating_list() {
        let cli = Cli {
            bots: 4,
            personalities: None,
            random_personalities: false,
            hud: false,
            layout: "table".to_string(),
            table: "default".to_string(),
            stack: 1500,
            sb: 5,
            bb: 10,
            hands: 0,
            name: "YOU".to_string(),
            quiet: false,
            no_anim: false,
            bot_think_ms: 1000,
            bot_bust_policy: "sitout".to_string(),
            replay_keep: 10,
            seed: None,
            history: None,
            skill: "regular".to_string(),
            skills: Some("rookie,pro".to_string()),
        };
        assert_eq!(
            cli.resolve_skills(),
            vec![
                SkillLevel::Rookie,
                SkillLevel::Pro,
                SkillLevel::Rookie,
                SkillLevel::Pro,
            ]
        );
    }

    #[test]
    fn fixed_personalities_use_preset_order() {
        let cli = Cli {
            bots: 3,
            personalities: None,
            random_personalities: false,
            hud: false,
            layout: "table".to_string(),
            table: "default".to_string(),
            stack: 1500,
            sb: 5,
            bb: 10,
            hands: 0,
            name: "YOU".to_string(),
            quiet: false,
            no_anim: false,
            bot_think_ms: 1000,
            bot_bust_policy: "sitout".to_string(),
            replay_keep: 10,
            seed: None,
            history: None,
            skill: "regular".to_string(),
            skills: None,
        };
        let labels: Vec<&str> = cli
            .resolve_personalities()
            .iter()
            .map(|p| p.label)
            .collect();
        assert_eq!(labels, vec!["Rock", "Shark", "Maniac"]);
    }
}
