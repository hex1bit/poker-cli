//! 一手牌的运行时状态。
//!
//! 仅持有 *单手牌* 的数据；多手之间的桌面级状态（按钮位、累计盈亏）
//! 由更上层的牌桌循环维护。

use crate::card::Card;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerStatus {
    /// 仍在牌局中，可以行动。
    Active,
    /// 已弃牌，本手出局。
    Folded,
    /// 已全押，无法再行动；仍可参与对应主/边池的摊牌。
    AllIn,
    /// 本手不参与（坐起）。
    SitOut,
}

#[derive(Debug, Clone)]
pub struct Player {
    pub name: String,
    pub stack: u64,
    pub hole: Option<[Card; 2]>,
    pub status: PlayerStatus,
    /// 本轮（preflop/flop/turn/river 任一）的累计下注。
    pub committed_round: u64,
    /// 整手牌累计下注（用于边池切分）。
    pub committed_total: u64,
    /// 本轮是否已经行动过一次（用于判断下注轮是否封闭）。
    pub acted_this_round: bool,
    /// 弃牌时秀的一张牌（"show one"）；新一手开始时重置。
    pub last_revealed: Option<Card>,
}

impl Player {
    pub fn new(name: impl Into<String>, stack: u64) -> Self {
        Self {
            name: name.into(),
            stack,
            hole: None,
            status: PlayerStatus::Active,
            committed_round: 0,
            committed_total: 0,
            acted_this_round: false,
            last_revealed: None,
        }
    }

    pub fn is_in_hand(&self) -> bool {
        matches!(self.status, PlayerStatus::Active | PlayerStatus::AllIn)
    }

    pub fn can_act(&self) -> bool {
        matches!(self.status, PlayerStatus::Active) && self.stack > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Preflop,
    Flop,
    Turn,
    River,
    Showdown,
    /// 本手已结束（包括所有人弃牌只剩一人的情形）。
    Complete,
}

impl Stage {
    /// 进入下一阶段。
    pub fn next(self) -> Stage {
        match self {
            Stage::Preflop => Stage::Flop,
            Stage::Flop => Stage::Turn,
            Stage::Turn => Stage::River,
            Stage::River => Stage::Showdown,
            Stage::Showdown => Stage::Complete,
            Stage::Complete => Stage::Complete,
        }
    }
}

/// 一手牌的完整状态。
#[derive(Debug, Clone)]
pub struct HandState {
    pub players: Vec<Player>,
    /// 按钮位的座位下标。
    pub button: usize,
    pub stage: Stage,
    /// 已发出的公共牌（0/3/4/5）。
    pub community: Vec<Card>,
    /// 剩余牌堆（栈顶在末尾，便于 pop）。
    pub deck: Vec<Card>,
    /// 本轮已知最高 commit 额。
    pub current_bet: u64,
    /// 下一次合法 raise 的最小 delta（默认等于 bb，被加注后等于上次加注的增量）。
    pub min_raise: u64,
    /// 当前应行动者的下标，若本轮已结束则为 None。
    pub to_act: Option<usize>,
    /// 本轮最近的进攻者（最近一次 bet/raise/all-in 加注者）。
    pub last_aggressor: Option<usize>,
    pub sb: u64,
    pub bb: u64,
}

impl HandState {
    /// 仍可摊牌的玩家数（Active + AllIn）。
    pub fn in_hand_count(&self) -> usize {
        self.players.iter().filter(|p| p.is_in_hand()).count()
    }

    /// 仍可继续行动的玩家数（Active 且有筹码）。
    pub fn can_act_count(&self) -> usize {
        self.players.iter().filter(|p| p.can_act()).count()
    }

    /// 主池 + 边池合计（不论结算与否，等于所有 committed_total 之和）。
    pub fn total_pot(&self) -> u64 {
        self.players.iter().map(|p| p.committed_total).sum()
    }

    /// 给定玩家需要 call 多少。
    pub fn to_call_for(&self, idx: usize) -> u64 {
        self.current_bet.saturating_sub(self.players[idx].committed_round)
    }
}
