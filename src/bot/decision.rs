//! Bot 决策核心：把 equity / pot_odds / 性格参数组合成具体 `Action`。

use rand::Rng;

use crate::bot::mood::Mood;
use crate::bot::personality::Personality;
use crate::bot::skill::SkillLevel;
use crate::card::{Card, Rank};
use crate::equity::{mc_equity, preflop_strength};
use crate::game::action::Action;
use crate::game::state::{HandState, PlayerStatus, Stage};
use crate::table::profile::TableRead;

/// 让一个 bot 在当前状态下决定动作（不带 mood，等价于 mood = default）。
///
/// 必须保证 `state.to_act == Some(seat)` 且该玩家为 Active。
pub fn decide<R: Rng + ?Sized>(
    state: &HandState,
    seat: usize,
    persona: Personality,
    rng: &mut R,
) -> Action {
    decide_with_mood_and_skill(
        state,
        seat,
        persona,
        SkillLevel::DEFAULT,
        &Mood::default(),
        rng,
    )
}

/// 带情绪的决策：tilt 上升时 effective aggression / vpip / bluff 也提升。
pub fn decide_with_mood<R: Rng + ?Sized>(
    state: &HandState,
    seat: usize,
    persona: Personality,
    mood: &Mood,
    rng: &mut R,
) -> Action {
    decide_with_mood_and_skill(state, seat, persona, SkillLevel::DEFAULT, mood, rng)
}

/// 带能力档位的决策，但不读取桌面画像。
pub fn decide_with_mood_and_skill<R: Rng + ?Sized>(
    state: &HandState,
    seat: usize,
    persona: Personality,
    skill: SkillLevel,
    mood: &Mood,
    rng: &mut R,
) -> Action {
    decide_with_mood_profile_skill(state, seat, persona, skill, mood, None, rng)
}

/// 带桌面画像的决策。画像来自已观察到的对手 VPIP/PFR/aggression，用于轻量调参。
pub fn decide_with_mood_and_profile<R: Rng + ?Sized>(
    state: &HandState,
    seat: usize,
    persona: Personality,
    mood: &Mood,
    table_read: Option<TableRead>,
    rng: &mut R,
) -> Action {
    decide_with_mood_profile_skill(
        state,
        seat,
        persona,
        SkillLevel::DEFAULT,
        mood,
        table_read,
        rng,
    )
}

/// 带桌面画像和能力档位的决策。
pub fn decide_with_mood_profile_skill<R: Rng + ?Sized>(
    state: &HandState,
    seat: usize,
    persona: Personality,
    skill: SkillLevel,
    mood: &Mood,
    table_read: Option<TableRead>,
    rng: &mut R,
) -> Action {
    let hole = state.players[seat]
        .hole
        .expect("bot needs hole cards to decide");
    let to_call = state.to_call_for(seat);
    let pot = state.total_pot();
    let stack = state.players[seat].stack;
    let opponents = state
        .players
        .iter()
        .enumerate()
        .filter(|(i, p)| *i != seat && p.is_in_hand())
        .count()
        .max(1);

    // mood 注入：tilt 上升 → 更激进、更松、更爱 bluff
    let tilt = mood.tilt;
    let read = table_read.unwrap_or_default();
    let profile_weight = skill.profile_weight();
    let table_loose = if read.samples == 0 {
        0.0
    } else {
        ((read.avg_vpip - 0.30) * profile_weight).clamp(-0.20, 0.30)
    };
    let table_aggressive = if read.samples == 0 {
        0.0
    } else {
        (((read.avg_aggression - 1.5) / 4.0) * profile_weight).clamp(-0.20, 0.30)
    };
    let table_raises = if read.samples == 0 {
        0.0
    } else {
        ((read.avg_pfr - 0.18) * profile_weight).clamp(-0.15, 0.25)
    };

    let eff_aggression =
        (persona.aggression * (1.0 + tilt) * (1.0 - table_aggressive * 0.20)).clamp(0.1, 6.0);
    let eff_vpip = (persona.vpip + tilt * 0.10 - table_raises * 0.10).clamp(0.0, 1.0);
    let eff_pfr = (persona.pfr + tilt * 0.10 - table_loose * 0.08).clamp(0.0, 1.0);
    let eff_bluff = (persona.bluff_freq + tilt * 0.10 - table_loose * 0.35).clamp(0.0, 1.0);
    let value_threshold_adjust = if read.samples == 0 {
        0.0
    } else {
        (-table_loose * 0.12).clamp(-0.04, 0.03)
    };

    // 1) 计算 equity。
    let mut equity = if state.stage == Stage::Preflop {
        preflop_strength(hole)
    } else {
        mc_equity(hole, &state.community, opponents, skill.mc_iters(), rng)
    };
    let noise = skill.equity_noise();
    if noise > 0.0 {
        equity = (equity + rng.gen_range(-noise..=noise)).clamp(0.0, 1.0);
    }

    // 2) 底池赔率（call 需要的最小 equity）。
    let pot_odds = if to_call == 0 {
        0.0
    } else {
        to_call as f64 / (pot + to_call) as f64
    };
    let edge = equity - pot_odds;

    // 3) 位置加权：晚位 (button 附近) 更松。
    let position_bonus = position_weight(state, seat, persona.position_aware);

    // ===== Preflop 入池决定 =====
    if state.stage == Stage::Preflop {
        let range = preflop_range_plan(hole, persona, position_bonus, to_call > 0);
        let effective_vpip = (eff_vpip + position_bonus * 0.10).clamp(0.0, 1.0);
        let raise_threshold = preflop_strength_threshold(eff_pfr);
        let call_threshold = preflop_strength_threshold(effective_vpip);

        // 自由 BB option：to_call == 0
        if to_call == 0 {
            if range.raise || equity >= raise_threshold || rng.r#gen::<f64>() < eff_bluff * 0.5 {
                return raise_size(state, seat, persona, equity, eff_aggression, rng);
            }
            return Action::Check;
        }

        // 需要付钱才能进入
        if range.raise || equity >= raise_threshold {
            // 强牌：偶尔慢玩
            if rng.r#gen::<f64>() < persona.slowplay_freq * 0.5 {
                return maybe_mistake(Action::Call, state, seat, rng, skill);
            }
            return maybe_mistake(
                raise_size(state, seat, persona, equity, eff_aggression, rng),
                state,
                seat,
                rng,
                skill,
            );
        }
        if range.call || equity >= call_threshold {
            // 中等：跟注
            return maybe_mistake(
                safe_call_or_fold(state, seat, to_call),
                state,
                seat,
                rng,
                skill,
            );
        }
        // 弱牌：偶尔诈唬，否则弃
        if rng.r#gen::<f64>() < eff_bluff * 0.5 && stack > to_call * 3 {
            return maybe_mistake(
                raise_size(state, seat, persona, equity, eff_aggression, rng),
                state,
                seat,
                rng,
                skill,
            );
        }
        return Action::Fold;
    }

    // ===== Postflop 决策 =====
    let is_aggressor_last_street = state.last_aggressor == Some(seat);
    let texture = board_texture(&state.community);
    let scary_board = texture.scariness;

    // 没人下注 → 选择 check / bet
    if to_call == 0 {
        // 强牌
        if equity > 0.65 + value_threshold_adjust {
            if rng.r#gen::<f64>() < persona.slowplay_freq {
                return Action::Check;
            }
            return maybe_mistake(
                open_bet(state, seat, persona, equity, eff_aggression, rng),
                state,
                seat,
                rng,
                skill,
            );
        }
        // 中等牌
        if equity > 0.45 + value_threshold_adjust {
            // 多人池子要更保守
            if opponents >= 3 && equity < 0.55 {
                return Action::Check;
            }
            // value bet 频率随 aggression 增长
            let bet_p = (0.35 * eff_aggression).clamp(0.0, 0.9);
            if rng.r#gen::<f64>() < bet_p {
                return maybe_mistake(
                    open_bet(state, seat, persona, equity, eff_aggression, rng),
                    state,
                    seat,
                    rng,
                    skill,
                );
            }
            return Action::Check;
        }
        // 弱牌：cbet 或 bluff
        let cbet_chance = if is_aggressor_last_street {
            (persona.cbet_freq + texture.cbet_adjust).clamp(0.05, 0.95)
        } else {
            eff_bluff * (1.0 + scary_board)
        };
        if rng.r#gen::<f64>() < cbet_chance {
            return maybe_mistake(
                open_bet(state, seat, persona, equity.max(0.30), eff_aggression, rng),
                state,
                seat,
                rng,
                skill,
            );
        }
        return Action::Check;
    }

    // 有人下注，需要决定 fold/call/raise
    // 强牌
    if edge > 0.20 {
        if rng.r#gen::<f64>() < persona.slowplay_freq * 0.5 {
            return maybe_mistake(
                safe_call_or_fold(state, seat, to_call),
                state,
                seat,
                rng,
                skill,
            );
        }
        return maybe_mistake(
            raise_size(state, seat, persona, equity, eff_aggression, rng),
            state,
            seat,
            rng,
            skill,
        );
    }
    // 有正期望
    if edge > 0.05 {
        // aggression 大 → 偏向加注
        let raise_p = (0.30 * eff_aggression).clamp(0.0, 0.85);
        if rng.r#gen::<f64>() < raise_p {
            return maybe_mistake(
                raise_size(state, seat, persona, equity, eff_aggression, rng),
                state,
                seat,
                rng,
                skill,
            );
        }
        return maybe_mistake(
            safe_call_or_fold(state, seat, to_call),
            state,
            seat,
            rng,
            skill,
        );
    }
    // 边际：偶尔跟
    if edge > -0.05 {
        return maybe_mistake(
            safe_call_or_fold(state, seat, to_call),
            state,
            seat,
            rng,
            skill,
        );
    }
    // 烂牌：偶尔诈唬加注
    if rng.r#gen::<f64>() < eff_bluff * (0.5 + 0.5 * scary_board) {
        return maybe_mistake(
            raise_size(state, seat, persona, 0.45, eff_aggression, rng),
            state,
            seat,
            rng,
            skill,
        );
    }
    Action::Fold
}

/// 由 `pfr` / `vpip` 估算对应的 preflop 强度阈值。
/// 简化：阈值 = 1 - frequency。频率越高，阈值越低。
fn preflop_strength_threshold(freq: f64) -> f64 {
    (1.0 - freq).clamp(0.30, 0.95)
}

/// 决定加注金额（既可能是 Bet，也可能是 Raise）。
///
/// 设计原则（避免动辄 all-in，让 bot 像人一样珍惜筹码）：
/// 1. 计算理论 target，按 equity / aggression / persona 缩放。
/// 2. Preflop 加注硬封 4× current_bet（除强加注/牌力外）。
/// 3. SPR 感知：深筹（SPR ≥ 4）下注上限收紧到 ~1×pot；极深（SPR ≥ 8）压到 0.85×pot。
/// 4. Jam 要满足谨慎条件之一才允许：短筹、强 equity + 低 SPR、已 commit 大量筹码、
///    或 ShortStacker 在低 BB 时的"短筹推"。其余情况下宁可缩小为非 all-in raise。
fn raise_size<R: Rng + ?Sized>(
    state: &HandState,
    seat: usize,
    persona: Personality,
    equity: f64,
    eff_aggression: f64,
    _rng: &mut R,
) -> Action {
    let pot = state.total_pot().max(state.bb);
    let texture = board_texture(&state.community);
    let bb = state.bb.max(1);
    let stack = state.players[seat].stack;
    let committed_round = state.players[seat].committed_round;
    let committed_total = state.players[seat].committed_total;
    let stack_bb = stack as f64 / bb as f64;
    let spr = stack as f64 / pot as f64;

    // 1) 基础 scale。
    let scale = ((equity - 0.35).max(0.0) * 1.5 + 0.5)
        * (0.7 + 0.3 * eff_aggression)
        * persona_sizing_factor(persona, state.stage, texture);

    let mut target_total: u64 = if state.current_bet == 0 {
        // open bet
        ((pot as f64) * scale.clamp(0.4, 1.2)) as u64
    } else {
        // raise to total
        let raise_amt = ((pot as f64) * scale.clamp(0.5, 1.5)) as u64;
        state.current_bet.saturating_add(raise_amt)
    };

    // 2) Preflop 硬封：面对 raise 时，4-bet 上限 = 4× current_bet（除非已是 4-bet+ 池子）。
    if state.stage == Stage::Preflop && state.current_bet > 0 {
        let preflop_cap = state.current_bet.saturating_mul(4).max(state.bb * 8);
        target_total = target_total.min(preflop_cap);
    }

    // 3) SPR 感知的硬上限（避免无脑变 jam）。
    let max_total_legal = committed_round.saturating_add(stack);
    let cap_factor: f64 = if spr >= 8.0 {
        0.85
    } else if spr >= 4.0 {
        1.20
    } else {
        2.00
    };
    let pot_based_cap: u64 = if state.current_bet == 0 {
        ((pot as f64) * cap_factor) as u64
    } else {
        state
            .current_bet
            .saturating_add(((pot as f64) * cap_factor) as u64)
    };
    target_total = target_total.min(pot_based_cap).min(max_total_legal);

    // 4) 合法性：>= bb；面对加注时 >= current_bet + min_raise。
    target_total = target_total.max(bb);
    if state.current_bet > 0 {
        let min_total = state.current_bet.saturating_add(state.min_raise);
        target_total = target_total.max(min_total);
    }

    // 5) Jam 决策。
    let need_pay = target_total.saturating_sub(committed_round);
    let already_committed_ratio =
        committed_total as f64 / (committed_total + stack).max(1) as f64;

    let jam = should_jam(JamCtx {
        persona,
        equity,
        stack_bb,
        spr,
        need_pay,
        stack,
        already_committed_ratio,
    });

    if need_pay >= stack {
        // 想 raise 但量超出 stack。
        if jam {
            return Action::AllIn;
        }
        // 不愿 jam → 缩到 ~50% stack 的非全押 raise。
        let safer_pay = (stack / 2).max(state.min_raise.max(bb));
        // 留至少 1 chip 不 all-in。
        let safer_pay = safer_pay.min(stack.saturating_sub(1)).max(1);
        target_total = committed_round.saturating_add(safer_pay);
        // 如果连最小加注门槛都凑不齐，退化为 call/check。
        if state.current_bet > 0
            && target_total < state.current_bet.saturating_add(state.min_raise)
        {
            return safe_call_or_fold(state, seat, state.to_call_for(seat));
        }
    }

    if state.current_bet == 0 {
        Action::Bet(target_total)
    } else {
        Action::Raise(target_total)
    }
}

/// Jam 决策上下文。
struct JamCtx {
    persona: Personality,
    equity: f64,
    stack_bb: f64,
    spr: f64,
    need_pay: u64,
    stack: u64,
    /// 在本手中已投入的筹码占总筹码（包括未投入）的比例。
    already_committed_ratio: f64,
}

/// 是否应该 all-in。基本准则：人类般谨慎，宁可少 raise 也不轻易 jam。
fn should_jam(c: JamCtx) -> bool {
    // ShortStacker 性格修订：短筹（≤ 25 BB）且 stack ≤ 1× pot 时才 jam。
    if c.persona.label == "ShortStacker" && c.stack_bb <= 25.0 && c.spr <= 1.0 {
        return c.equity > 0.45;
    }
    // 极强牌：equity 巨大，直接干掉。
    if c.equity > 0.92 {
        return true;
    }
    // 短筹推：≤ 15 BB 且 equity 不烂。
    if c.stack_bb <= 15.0 && c.equity > 0.50 {
        return true;
    }
    // 已经 pot-committed：本手已投入 ≥ 35% 总筹码 + equity 不烂 → 不退缩。
    if c.already_committed_ratio > 0.35 && c.equity > 0.55 {
        return true;
    }
    // 浅 SPR 强牌：SPR < 3 且 equity > 0.78 → value jam。
    if c.spr < 3.0 && c.equity > 0.78 {
        return true;
    }
    // 极浅 SPR + 中等牌。
    if c.spr < 1.5 && c.equity > 0.55 {
        return true;
    }
    // 加注金额本身只占 stack 一小部分（不算 all-in）→ 走 raise，不 jam。
    if (c.need_pay as f64) < (c.stack as f64) * 0.7 {
        return false;
    }
    // 其余情况：默认不 jam。
    false
}

/// 没有 "开桌下注" 选项时（已有 bet/raise）：决定 call / fold。
fn safe_call_or_fold(state: &HandState, seat: usize, to_call: u64) -> Action {
    let stack = state.players[seat].stack;
    if to_call == 0 {
        Action::Check
    } else if to_call >= stack {
        Action::AllIn
    } else {
        Action::Call
    }
}

fn maybe_mistake<R: Rng + ?Sized>(
    action: Action,
    state: &HandState,
    seat: usize,
    rng: &mut R,
    skill: SkillLevel,
) -> Action {
    if rng.r#gen::<f64>() >= skill.mistake_rate() {
        return action;
    }
    let to_call = state.to_call_for(seat);
    if to_call == 0 {
        Action::Check
    } else if matches!(action, Action::Fold) {
        safe_call_or_fold(state, seat, to_call)
    } else {
        Action::Fold
    }
}

/// open bet 包装。
fn open_bet<R: Rng + ?Sized>(
    state: &HandState,
    seat: usize,
    persona: Personality,
    equity: f64,
    eff_aggression: f64,
    rng: &mut R,
) -> Action {
    raise_size(state, seat, persona, equity, eff_aggression, rng)
}

/// 位置加权 ∈ [-0.1, 0.1]：越靠近按钮位返回越大。
fn position_weight(state: &HandState, seat: usize, awareness: f64) -> f64 {
    let n = state.players.len();
    let active: Vec<usize> = (0..n)
        .filter(|&i| state.players[i].status != PlayerStatus::SitOut)
        .collect();
    if active.is_empty() {
        return 0.0;
    }
    // 距离按钮位的 “逆时针距离”：button 自身=0, button-1=1, ...
    let dist = (state.button + n - seat) % n;
    let rel = dist as f64 / n as f64; // 0..~1
    // 0 (button) → +0.10 * awareness; 远离 → 负
    let w = (0.10 - rel * 0.20) * awareness;
    w.clamp(-0.10, 0.10)
}

#[derive(Debug, Clone, Copy, Default)]
struct BoardTexture {
    scariness: f64,
    cbet_adjust: f64,
    wet_sizing: f64,
}

/// 板面 texture：干燥牌面更适合小额 cbet，湿润牌面更需要保护和谨慎诈唬。
fn board_texture(board: &[crate::card::Card]) -> BoardTexture {
    if board.len() < 3 {
        return BoardTexture::default();
    }
    let mut suits = [0u8; 4];
    for c in board {
        suits[c.suit().as_u8() as usize] += 1;
    }
    let max_suit = *suits.iter().max().unwrap();
    let flush_threat = match max_suit {
        4..=5 => 0.5,
        3 => 0.25,
        _ => 0.0,
    };
    let paired = board
        .iter()
        .enumerate()
        .any(|(i, c)| board.iter().skip(i + 1).any(|o| c.rank() == o.rank()));
    // 简单连通度：rank 集合排序后看最大窗口内 5 个 rank 中有多少
    let mut ranks: Vec<u8> = board.iter().map(|c| c.rank().as_u8()).collect();
    ranks.sort_unstable();
    ranks.dedup();
    let mut max_in_window = 0;
    for top in 4..=12u8 {
        let lo = top.saturating_sub(4);
        let cnt = ranks.iter().filter(|&&r| r >= lo && r <= top).count();
        if cnt > max_in_window {
            max_in_window = cnt;
        }
    }
    let straight_threat = (max_in_window as f64 - 2.0).max(0.0) * 0.15;
    let scariness = (flush_threat + straight_threat).clamp(0.0, 1.0);
    let wet_sizing = (1.0 + scariness * 0.35).clamp(1.0, 1.35);
    let cbet_adjust = if paired {
        0.08
    } else if scariness < 0.25 {
        0.10
    } else if scariness > 0.55 {
        -0.12
    } else {
        0.0
    };
    BoardTexture {
        scariness,
        cbet_adjust,
        wet_sizing,
    }
}

fn persona_sizing_factor(persona: Personality, stage: Stage, texture: BoardTexture) -> f64 {
    let base = match persona.label {
        "Nit" | "WeakTight" => 0.82,
        "Rock" => 0.90,
        "Station" | "Fish" => 0.75,
        "Maniac" | "Gambler" => 1.25,
        "Bluffer" => 1.15,
        "ShortStacker" => 1.35,
        "Pro" | "BalancedReg" | "Shark" => 1.0,
        _ => 1.0,
    };
    let street = if stage == Stage::Preflop {
        0.95
    } else {
        texture.wet_sizing
    };
    (base * street).clamp(0.55, 1.60)
}

#[derive(Debug, Clone, Copy)]
struct PreflopPlan {
    raise: bool,
    call: bool,
}

fn preflop_range_plan(
    hole: [Card; 2],
    persona: Personality,
    position_bonus: f64,
    facing_raise: bool,
) -> PreflopPlan {
    let score = preflop_range_score(hole);
    let late_bonus = (position_bonus * 2.0).clamp(-0.10, 0.18);
    let (open, defend) = match persona.label {
        "Nit" => (0.86, 0.80),
        "Rock" | "WeakTight" => (0.78, 0.70),
        "Shark" | "Pro" | "BalancedReg" => (0.66, 0.58),
        "Trapper" => (0.72, 0.64),
        "Bluffer" => (0.58, 0.50),
        "Maniac" | "Gambler" => (0.46, 0.40),
        "Fish" | "Station" => (0.82, 0.42),
        "ShortStacker" => (0.70, 0.62),
        _ => (0.68, 0.58),
    };
    let open_threshold = (open - late_bonus).clamp(0.25, 0.95);
    let defend_threshold = (defend - late_bonus * 0.5).clamp(0.25, 0.92);
    if facing_raise {
        PreflopPlan {
            raise: score >= (defend_threshold + 0.22).clamp(0.65, 0.97),
            call: score >= defend_threshold,
        }
    } else {
        PreflopPlan {
            raise: score >= open_threshold,
            call: score >= defend_threshold,
        }
    }
}

fn preflop_range_score(hole: [Card; 2]) -> f64 {
    let (hi, lo) = ordered_ranks(hole);
    let suited = hole[0].suit() == hole[1].suit();
    let pair = hi == lo;
    if pair {
        return 0.52 + hi.as_u8() as f64 / 12.0 * 0.48;
    }
    let gap = hi.as_u8() - lo.as_u8();
    let broadway = hi >= Rank::Ten && lo >= Rank::Ten;
    let ace = hi == Rank::Ace;
    let connector = gap <= 2;
    let mut score = 0.18 + hi.as_u8() as f64 / 12.0 * 0.34 + lo.as_u8() as f64 / 12.0 * 0.16;
    if suited {
        score += 0.10;
    }
    if broadway {
        score += 0.10;
    }
    if ace {
        score += if suited { 0.09 } else { 0.04 };
    }
    if connector {
        score += 0.07;
    } else if gap <= 4 && suited {
        score += 0.03;
    }
    if gap >= 6 && !ace {
        score -= 0.08;
    }
    score.clamp(0.0, 1.0)
}

fn ordered_ranks(hole: [Card; 2]) -> (Rank, Rank) {
    if hole[0].rank() >= hole[1].rank() {
        (hole[0].rank(), hole[1].rank())
    } else {
        (hole[1].rank(), hole[0].rank())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Card, Rank, Suit};
    use crate::game::betting::{apply_action, start_hand};
    use crate::game::state::Player;
    use rand::SeedableRng;

    fn aa(seat: usize) -> [Card; 2] {
        let s = if seat == 0 {
            (Suit::Hearts, Suit::Spades)
        } else {
            (Suit::Clubs, Suit::Diamonds)
        };
        [Card::new(Rank::Ace, s.0), Card::new(Rank::Ace, s.1)]
    }

    #[test]
    fn rock_folds_trash_preflop_facing_raise() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let mut s = start_hand(
            vec![
                Player::new("A", 1000),
                Player::new("B", 1000),
                Player::new("C", 1000),
            ],
            0,
            5,
            10,
            &mut rng,
        );
        // 强制 seat 0 是 ROCK，给 trash 7-2 offsuit；面对 raise
        s.players[0].hole = Some([
            Card::new(Rank::Seven, Suit::Hearts),
            Card::new(Rank::Two, Suit::Diamonds),
        ]);
        // 模拟前面有人 raise to 60
        // 当前 to_act 应该是 0（3-handed: UTG=0）
        // 手动制造一个 “面对加注” 的情况：先让 0 fold (no), 我们改方式 —— 直接调高 current_bet
        s.current_bet = 60;
        // ROCK 应弃
        let a = decide(&s, 0, Personality::ROCK, &mut rng);
        assert_eq!(a, Action::Fold);
    }

    #[test]
    fn maniac_raises_premium() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(2);
        let mut s = start_hand(
            vec![
                Player::new("A", 1000),
                Player::new("B", 1000),
                Player::new("C", 1000),
            ],
            0,
            5,
            10,
            &mut rng,
        );
        s.players[0].hole = Some(aa(0));
        let a = decide(&s, 0, Personality::MANIAC, &mut rng);
        // AA 几乎肯定 raise 或 all-in
        assert!(matches!(a, Action::Raise(_) | Action::AllIn));
    }

    #[test]
    fn fish_calls_with_marginal() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let mut s = start_hand(
            vec![
                Player::new("A", 1000),
                Player::new("B", 1000),
                Player::new("C", 1000),
            ],
            0,
            5,
            10,
            &mut rng,
        );
        // 给 Fish 一个边际牌 KJ 同花
        s.players[0].hole = Some([
            Card::new(Rank::King, Suit::Hearts),
            Card::new(Rank::Jack, Suit::Hearts),
        ]);
        s.current_bet = 30; // 有人 raise 了
        let a = decide(&s, 0, Personality::FISH, &mut rng);
        // Fish vpip 高，应至少 call（也可能因为强度高而 raise）
        assert!(matches!(a, Action::Call | Action::Raise(_) | Action::AllIn));
        let _ = apply_action; // silence unused
    }

    #[test]
    fn bot_vs_bot_runs_to_completion() {
        // 跑一手 6 个 bot vs bot, 验证不 panic 且 stage 抵达 Complete/Showdown 且筹码守恒
        let mut rng = rand::rngs::StdRng::seed_from_u64(101);
        let players: Vec<Player> = (0..6)
            .map(|i| Player::new(format!("Bot{i}"), 1000))
            .collect();
        let initial_total: u64 = players.iter().map(|p| p.stack).sum();
        let personas = Personality::PRESETS;

        let mut s = start_hand(players, 0, 5, 10, &mut rng);
        use crate::game::betting::{advance_street, round_closed};
        use crate::game::showdown::settle;

        let mut safety = 0usize;
        while s.stage != Stage::Complete && s.stage != Stage::Showdown {
            if let Some(seat) = s.to_act {
                let p = personas[seat % personas.len()];
                let a = decide(&s, seat, p, &mut rng);
                apply_action(&mut s, seat, a).expect("bot picks legal action");
            } else if round_closed(&s) {
                advance_street(&mut s);
            }
            safety += 1;
            assert!(safety < 500, "infinite loop?");
        }
        if s.stage == Stage::Showdown {
            settle(&mut s);
        } else {
            // 单人剩下 —— 直接结算
            settle(&mut s);
        }
        let final_total: u64 = s.players.iter().map(|p| p.stack).sum();
        assert_eq!(final_total, initial_total, "chip conservation broken");
    }
}
