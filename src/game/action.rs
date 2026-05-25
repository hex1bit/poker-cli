//! 玩家可以提交的动作。
//!
//! 这里使用 “本轮累计 commit 金额” 作为 raise/bet 的语义，方便与引擎里 `current_bet`
//! 直接比较；`apply_action` 会在内部转成 delta 扣筹码。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 弃牌。
    Fold,
    /// 过牌（仅当 `to_call == 0` 时合法）。
    Check,
    /// 跟注到当前最高 bet（不足部分自动转 all-in）。
    Call,
    /// 首次下注，本轮承诺 `amount`（仅当 `current_bet == 0` 时合法）。
    Bet(u64),
    /// 加注，将本轮承诺提到 `to_total` 这个总额。
    Raise(u64),
    /// 全押。
    AllIn,
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Fold => write!(f, "fold"),
            Action::Check => write!(f, "check"),
            Action::Call => write!(f, "call"),
            Action::Bet(a) => write!(f, "bet {a}"),
            Action::Raise(a) => write!(f, "raise to {a}"),
            Action::AllIn => write!(f, "all-in"),
        }
    }
}
