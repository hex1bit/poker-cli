//! CLI 参数与游戏配置。

use clap::Parser;

use crate::bot::Personality;

#[derive(Parser, Debug)]
#[command(name = "poker", version, about = "Texas Hold'em CLI vs AI bots")]
pub struct Cli {
    /// AI Bot 数量 (1..=6)。
    #[arg(long, default_value_t = 5)]
    pub bots: usize,
    /// 显式指定 Bot 性格列表，逗号分隔；如 "Rock,Shark,Maniac"。
    #[arg(long)]
    pub personalities: Option<String>,
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
    /// 保留多少手历史用于回放，默认 10。
    #[arg(long, default_value_t = 10)]
    pub replay_keep: usize,
}

impl Cli {
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
                return chosen.into_iter().take(self.bots).collect();
            }
        }
        Personality::PRESETS
            .iter()
            .copied()
            .take(self.bots.clamp(1, 6))
            .collect()
    }
}
