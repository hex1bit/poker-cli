//! Texas Hold'em CLI 主入口。

use std::io::ErrorKind;

use clap::Parser;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};

use poker::bot::{
    Mood, Personality, VoiceEvent,
    decision::decide_with_mood_profile_skill,
    names::{name_pool, sample_table_names},
    pick_line,
};
use poker::card::Card;
use poker::config::{BotBustPolicy, Cli};
use poker::game::action::Action;
use poker::game::betting::{advance_street, apply_action, round_closed, start_hand};
use poker::game::history::HandHistory;
use poker::game::showdown::settle;
use poker::game::state::{Player, PlayerStatus, Stage};
use poker::hand_eval::describe_7_with_best;
use poker::table::log::HandLogRecord;
use poker::table::profile::TableProfiles;
use poker::table::runner::advance_button;
use poker::ui::anim::{DealAnimationOptions, animate_deal_community, pause, pause_interruptible};
use poker::ui::input::prompt_action;
use poker::ui::render::{
    ContinueAction, RenderOptions, enter_screen, leave_screen, render, render_showdown,
    wait_for_continue,
};

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        // 确保终端被恢复
        let _ = leave_screen();
        if e.kind() != ErrorKind::Interrupted {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn run(cli: Cli) -> std::io::Result<()> {
    if let Err(msg) = cli.validate() {
        return Err(std::io::Error::new(ErrorKind::InvalidInput, msg));
    }
    let mut rng = match cli.seed {
        Some(seed) => StdRng::seed_from_u64(seed),
        None => StdRng::from_entropy(),
    };
    let mut personalities = cli.resolve_personalities_with_rng(&mut rng);
    let skills = cli.resolve_skills();
    let layout = cli.layout();
    let bot_bust_policy = cli.bot_bust_policy();
    let n_bots = personalities.len();
    if !(1..=9).contains(&n_bots) {
        return Err(std::io::Error::new(
            ErrorKind::InvalidInput,
            "bots must be 1..=9",
        ));
    }

    // 座位 0 = 真人，座位 1..=N = bots。
    let mut players: Vec<Player> = Vec::with_capacity(n_bots + 1);
    players.push(Player::new(cli.name.clone(), cli.stack));
    let bot_names = sample_table_names(&personalities, &mut rng);
    for (i, _p) in personalities.iter().enumerate() {
        players.push(Player::new(bot_names[i].clone(), cli.stack));
    }
    let hero_seat = 0usize;
    let mut button = personalities.len(); // 第一手按钮位放在最右 bot 上，让真人是 SB（头对头时为 button）
    // 每个座位一份 mood（hero 也有，但不会读取）
    let mut moods: Vec<Mood> = (0..=n_bots).map(|_| Mood::new()).collect();
    let mut profiles = TableProfiles::new(n_bots + 1);
    let mut history: std::collections::VecDeque<HandHistory> =
        std::collections::VecDeque::with_capacity(cli.replay_keep.max(1));
    let mut log: Vec<String> = Vec::new();
    log.push(format!(
        "Welcome, {}! {} bots: {}",
        cli.name,
        n_bots,
        personalities
            .iter()
            .enumerate()
            .map(|(i, p)| format!("{} ({} / {})", bot_names[i], p.label, skills[i]))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    let mut session = SessionStats::new(cli.stack);

    enter_screen()?;

    let mut hand_idx: u32 = 0;
    let result = loop {
        // 检查只剩一人：游戏结束
        handle_busted_bots(
            bot_bust_policy,
            BotBustEnv {
                players: &mut players,
                personalities: &mut personalities,
                moods: &mut moods,
                profiles: &mut profiles,
                initial_stack: cli.stack,
                rng: &mut rng,
                log: &mut log,
            },
        );
        let alive: Vec<usize> = (0..players.len())
            .filter(|&i| players[i].stack > 0)
            .collect();
        if alive.len() <= 1 {
            log.push(format!(
                "Game over. Winner: {}",
                if alive.is_empty() {
                    "<none>".to_string()
                } else {
                    players[alive[0]].name.clone()
                }
            ));
            // 渲染最后一帧（用一个空状态 fallback）—— 简化：直接打印 log
            break Ok(());
        }
        // 真人破产则结束
        if players[hero_seat].stack == 0 {
            log.push(format!("{} busted. Game over.", players[hero_seat].name));
            break Ok(());
        }
        // 限制手数
        if cli.hands > 0 && hand_idx >= cli.hands {
            log.push(format!("Hand limit ({}) reached. Game over.", cli.hands));
            break Ok(());
        }

        // 旋转按钮位到下一位有筹码的玩家
        button = advance_button(&players, button);

        // 开新一手
        let mut state = start_hand(players.clone(), button, cli.sb, cli.bb, &mut rng);
        let hand_log_start = log.len();
        profiles.start_hand(&state);
        log.push(format!(
            "── Hand #{} (button = {}) ──",
            hand_idx + 1,
            players[button].name
        ));
        log.push(format!("Blinds posted. Pot ${}.", state.total_pot()));
        let tilt_marks: Vec<bool> = moods.iter().map(|m| m.is_tilted()).collect();
        let hud = hud_lines(&profiles, cli.hud, state.players.len());
        render(
            &state,
            RenderOptions {
                hero_seat: Some(hero_seat),
                log: &log,
                reveal_all: false,
                winners: &[],
                tilt_marks: &tilt_marks,
                hud: hud.as_deref(),
                layout,
            },
        )?;
        let mut hand_history = HandHistory::new();
        hand_history.push(&state, log.len());

        // 进行下注/发牌循环
        loop {
            // 处理动作或推进街
            if let Some(seat) = state.to_act {
                let action_stage = state.stage;
                let action = if seat == hero_seat {
                    match prompt_action(&state, seat) {
                        Ok(a) => a,
                        Err(e) if e.kind() == ErrorKind::Interrupted => {
                            break_game_with_msg(&mut log, "User quit.");
                            return finish(log, build_session_summary(&session, &state.players, &profiles, hero_seat));
                        }
                        Err(e) => return Err(e),
                    }
                } else {
                    if cli.bot_think_ms > 0 && !cli.no_anim {
                        log.push(format!("{} 正在思考...", state.players[seat].name));
                        let tilt_marks: Vec<bool> = moods.iter().map(|m| m.is_tilted()).collect();
                        let hud = hud_lines(&profiles, cli.hud, state.players.len());
                        render(
                            &state,
                            RenderOptions {
                                hero_seat: Some(hero_seat),
                                log: &log,
                                reveal_all: false,
                                winners: &[],
                                tilt_marks: &tilt_marks,
                                hud: hud.as_deref(),
                                layout,
                            },
                        )?;
                        if pause_interruptible(cli.bot_think_ms)? {
                            break_game_with_msg(&mut log, "User quit.");
                            return finish(log, build_session_summary(&session, &players, &profiles, hero_seat));
                        }
                    }
                    let p = personalities[seat - 1];
                    let skill = skills[seat - 1];
                    let table_read = profiles.table_read_for(seat, &state);
                    decide_with_mood_profile_skill(
                        &state,
                        seat,
                        p,
                        skill,
                        &moods[seat],
                        Some(table_read),
                        &mut rng,
                    )
                };

                match apply_action(&mut state, seat, action) {
                    Ok(()) => {
                        profiles.observe_action(action_stage, seat, action);
                        log.push(format!("{} {}", state.players[seat].name, action));
                        // 触发台词（仅 bot）
                        if seat != hero_seat && !cli.quiet {
                            let p = personalities[seat - 1];
                            let ev = action_to_event(action, state.stage);
                            if let Some(ev) = ev
                                && let Some(line) = pick_line(&p, ev, &mut rng)
                            {
                                log.push(format!("{}：「{}」", state.players[seat].name, line));
                            }
                        }
                        // Show one card：bot 弃牌 → 按 show_freq 概率秀一张牌
                        if action == Action::Fold && seat != hero_seat {
                            let p = personalities[seat - 1];
                            if rng.r#gen::<f64>() < p.show_freq
                                && let Some(hole) = state.players[seat].hole
                            {
                                let pick = if rng.r#gen::<f64>() < 0.5 {
                                    hole[0]
                                } else {
                                    hole[1]
                                };
                                state.players[seat].last_revealed = Some(pick);
                                log.push(format!(
                                    "{} 秀: [{}{}]",
                                    state.players[seat].name,
                                    pick.rank(),
                                    pick.suit()
                                ));
                            }
                        }
                    }
                    Err(msg) => {
                        // 非法动作（不应该发生于 bot；真人输入若不合法这里退化为 fold/check）。
                        log.push(format!(
                            "{} attempted illegal action ({}); folded.",
                            state.players[seat].name, msg
                        ));
                        let _ = apply_action(&mut state, seat, Action::Fold);
                    }
                }
                let tilt_marks: Vec<bool> = moods.iter().map(|m| m.is_tilted()).collect();
                let hud = hud_lines(&profiles, cli.hud, state.players.len());
                render(
                    &state,
                    RenderOptions {
                        hero_seat: Some(hero_seat),
                        log: &log,
                        reveal_all: false,
                        winners: &[],
                        tilt_marks: &tilt_marks,
                        hud: hud.as_deref(),
                        layout,
                    },
                )?;
                hand_history.push(&state, log.len());
            } else if round_closed(&state) {
                // 切下一街
                if state.stage == Stage::Preflop
                    || state.stage == Stage::Flop
                    || state.stage == Stage::Turn
                    || state.stage == Stage::River
                {
                    let prev_stage = state.stage;
                    advance_street(&mut state);
                    if state.stage == Stage::Flop
                        || state.stage == Stage::Turn
                        || state.stage == Stage::River
                    {
                        log.push(format!("── {} ──", state.stage.label_zh()));
                        let tilt_marks: Vec<bool> = moods.iter().map(|m| m.is_tilted()).collect();
                        let new_cards = match prev_stage {
                            Stage::Preflop => 3, // flop
                            _ => 1,              // turn / river
                        };
                        let delay = if cli.no_anim { 0 } else { 220 };
                        let hud = hud_lines(&profiles, cli.hud, state.players.len());
                        animate_deal_community(
                            &state,
                            DealAnimationOptions {
                                hero: Some(hero_seat),
                                log: &log,
                                tilt_marks: &tilt_marks,
                                hud: hud.as_deref(),
                                layout,
                                new_card_count: new_cards,
                                delay_ms: delay,
                            },
                        )?;
                        hand_history.push(&state, log.len());
                    }
                }
                if state.stage == Stage::Showdown || state.stage == Stage::Complete {
                    break;
                }
            } else {
                // 不应该到这里
                break;
            }
        }

        // 摊牌或单人胜出 → 结算
        let in_hand: Vec<usize> = (0..state.players.len())
            .filter(|&i| state.players[i].is_in_hand())
            .collect();
        let pot = state.total_pot();
        let delta = settle(&mut state);
        // 计算赢家（拿到 ≥ 1 chip 的人）
        let winners: Vec<usize> = delta
            .iter()
            .enumerate()
            .filter(|(_, d)| **d > 0)
            .map(|(i, _)| i)
            .collect();
        if in_hand.len() == 1 {
            log.push(format!(
                "{} wins ${} uncontested.",
                state.players[in_hand[0]].name, pot
            ));
            // 台词：无对抗收获
            let winner = in_hand[0];
            if winner != hero_seat && !cli.quiet {
                let p = personalities[winner - 1];
                if let Some(line) = pick_line(&p, VoiceEvent::WonUncontested, &mut rng) {
                    log.push(format!("{}：「{}」", state.players[winner].name, line));
                }
            }
            let tilt_marks: Vec<bool> = moods.iter().map(|m| m.is_tilted()).collect();
            let hud = hud_lines(&profiles, cli.hud, state.players.len());
            render(
                &state,
                RenderOptions {
                    hero_seat: Some(hero_seat),
                    log: &log,
                    reveal_all: false,
                    winners: &winners,
                    tilt_marks: &tilt_marks,
                    hud: hud.as_deref(),
                    layout,
                },
            )?;
        } else {
            let winner_names = winners
                .iter()
                .map(|&i| state.players[i].name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            log.push(format!("Showdown result: winner(s) = {winner_names}"));
            for (i, d) in delta.iter().enumerate().take(state.players.len()) {
                if state.players[i].status != PlayerStatus::Folded
                    && let Some(h) = state.players[i].hole
                {
                    let hand_desc = if state.community.len() == 5 {
                        let (desc, best) = describe_7_with_best(&[
                            h[0],
                            h[1],
                            state.community[0],
                            state.community[1],
                            state.community[2],
                            state.community[3],
                            state.community[4],
                        ]);
                        format!(" — {}，最佳五张: {}", desc, format_cards(&best))
                    } else {
                        String::new()
                    };
                    let result = if *d > 0 {
                        format!("+${d}")
                    } else if *d < 0 {
                        format!("-${}", d.abs())
                    } else {
                        "$0".to_string()
                    };
                    let winner_mark = if winners.contains(&i) { " WIN" } else { "" };
                    log.push(format!(
                        "  {}: [{}] [{}]{} => {}{}",
                        state.players[i].name, h[0], h[1], hand_desc, result, winner_mark
                    ));
                }
            }
            let tilt_marks: Vec<bool> = moods.iter().map(|m| m.is_tilted()).collect();
            let hud = hud_lines(&profiles, cli.hud, state.players.len());
            render_showdown(&state, &log, &winners, &tilt_marks, hud.as_deref(), layout)?;
            // 给赢家高亮一点呼吸时间
            if !cli.no_anim {
                pause(700);
            }
            // 摊牌台词：根据 delta 判定胜负
            if !cli.quiet {
                for (i, d) in delta.iter().enumerate() {
                    if i == hero_seat {
                        continue;
                    }
                    if state.players[i].status == PlayerStatus::Folded {
                        continue;
                    }
                    let p = personalities[i - 1];
                    let ev = if *d > 0 {
                        VoiceEvent::WonShowdown
                    } else {
                        VoiceEvent::LostShowdown
                    };
                    if let Some(line) = pick_line(&p, ev, &mut rng) {
                        log.push(format!("{}：「{}」", state.players[i].name, line));
                    }
                }
            }
        }
        // 更新各 bot mood，并在新进入 tilt 时触发台词
        for i in 1..=n_bots {
            let was_tilted = moods[i].is_tilted();
            let p = personalities[i - 1];
            moods[i].after_hand(delta[i], p.tilt_factor);
            if delta[i] < -((state.bb * 10) as i64) && winners.iter().any(|&w| w != i) {
                moods[i].tilt = (moods[i].tilt + 0.12 * (1.0 + p.tilt_factor)).clamp(0.0, 1.0);
            }
            let now_tilted = moods[i].is_tilted();
            if !was_tilted
                && now_tilted
                && !cli.quiet
                && let Some(line) = pick_line(&p, VoiceEvent::TiltOn, &mut rng)
            {
                log.push(format!("{}：「{}」", state.players[i].name, line));
            }
        }
        profiles.finish_hand(&state, &winners);
        session.record(delta[hero_seat], winners.contains(&hero_seat));

        // 保存最终一帧 + 赢家
        hand_history.push(&state, log.len());
        hand_history.winners = winners.clone();
        hand_history.final_log_len = log.len();
        hand_history.set_logs(&log, hand_log_start);

        if let Some(path) = &cli.history {
            let record = HandLogRecord::from_state(
                hand_idx + 1,
                &state,
                &winners,
                &delta,
                &log[hand_log_start..],
            );
            record.append_jsonl(path)?;
        }

        if history.len() == cli.replay_keep && cli.replay_keep > 0 {
            history.pop_front();
        }
        if cli.replay_keep > 0 {
            history.push_back(hand_history);
        }

        // 主循环：可能反复 replay 直到用户按 space
        loop {
            let prompt = if !history.is_empty() {
                "Press <space> next, [R] replay, q/Esc quit"
            } else {
                "Press <space> next, q/Esc quit"
            };
            match wait_for_continue(prompt) {
                Ok(ContinueAction::Next) => break,
                Ok(ContinueAction::Replay) => {
                    if let Some(last) = history.back() {
                        replay_hand(last, hero_seat, &moods, cli.no_anim, layout)?;
                        // 回放结束后重绘当前桌面，再次提示
                        let tilt_marks: Vec<bool> = moods.iter().map(|m| m.is_tilted()).collect();
                        let hud = hud_lines(&profiles, cli.hud, state.players.len());
                        render(
                            &state,
                            RenderOptions {
                                hero_seat: Some(hero_seat),
                                log: &log,
                                reveal_all: true,
                                winners: &winners,
                                tilt_marks: &tilt_marks,
                                hud: hud.as_deref(),
                                layout,
                            },
                        )?;
                    }
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {
                    break_game_with_msg(&mut log, "User quit.");
                    return finish(log, build_session_summary(&session, &players, &profiles, hero_seat));
                }
                Err(e) => return Err(e),
            }
        }

        // 把筹码写回主玩家列表
        players = state.players;

        hand_idx += 1;
    };

    leave_screen()?;
    for l in build_session_summary(&session, &players, &profiles, hero_seat) {
        println!("{l}");
    }
    println!();
    for l in log {
        println!("{l}");
    }
    result
}

fn finish(log: Vec<String>, session_summary: Vec<String>) -> std::io::Result<()> {
    leave_screen()?;
    for l in session_summary {
        println!("{l}");
    }
    println!();
    for l in log {
        println!("{l}");
    }
    Ok(())
}

fn break_game_with_msg(log: &mut Vec<String>, msg: &str) {
    log.push(msg.to_string());
}

struct BotBustEnv<'a, R: Rng + ?Sized> {
    players: &'a mut [Player],
    personalities: &'a mut [Personality],
    moods: &'a mut [Mood],
    profiles: &'a mut TableProfiles,
    initial_stack: u64,
    rng: &'a mut R,
    log: &'a mut Vec<String>,
}

fn handle_busted_bots<R: Rng + ?Sized>(policy: BotBustPolicy, env: BotBustEnv<'_, R>) {
    if policy == BotBustPolicy::SitOut {
        return;
    }
    for seat in 1..env.players.len() {
        if env.players[seat].stack > 0 {
            continue;
        }
        match policy {
            BotBustPolicy::SitOut => {}
            BotBustPolicy::Rebuy => {
                env.players[seat].stack = env.initial_stack;
                env.players[seat].status = PlayerStatus::Active;
                env.moods[seat] = Mood::new();
                env.log.push(format!(
                    "{} rebuy to ${}.",
                    env.players[seat].name, env.initial_stack
                ));
            }
            BotBustPolicy::Replace => {
                let old_name = env.players[seat].name.clone();
                let persona = *Personality::PRESETS
                    .choose(env.rng)
                    .unwrap_or(&Personality::BALANCED_REG);
                env.personalities[seat - 1] = persona;
                let name = replacement_name(persona, env.players, seat, env.rng);
                env.players[seat] = Player::new(name.clone(), env.initial_stack);
                env.moods[seat] = Mood::new();
                env.profiles.reset_seat(seat);
                env.log.push(format!(
                    "{old_name} leaves the table. {name} ({}) takes seat #{seat} with ${}.",
                    persona.label, env.initial_stack
                ));
            }
        }
    }
}

fn replacement_name<R: Rng + ?Sized>(
    persona: Personality,
    players: &[Player],
    seat: usize,
    rng: &mut R,
) -> String {
    let used: Vec<&str> = players
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != seat)
        .map(|(_, p)| p.name.as_str())
        .collect();
    let mut pool = name_pool(&persona).to_vec();
    pool.shuffle(rng);
    for name in pool {
        if !used.contains(&name) {
            return name.to_string();
        }
    }
    format!("{}#{}", persona.label, seat)
}

fn hud_lines(profiles: &TableProfiles, enabled: bool, seats: usize) -> Option<Vec<String>> {
    enabled.then(|| profiles.hud_lines(seats))
}

fn format_cards(cards: &[Card; 5]) -> String {
    cards
        .iter()
        .map(|c| format!("[{c}]"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 真人本场战绩。
struct SessionStats {
    hands_played: u32,
    hands_won: u32,
    starting_stack: u64,
    biggest_win: i64,
    biggest_loss: i64,
}

impl SessionStats {
    fn new(starting_stack: u64) -> Self {
        Self {
            hands_played: 0,
            hands_won: 0,
            starting_stack,
            biggest_win: 0,
            biggest_loss: 0,
        }
    }

    fn record(&mut self, delta: i64, won: bool) {
        self.hands_played += 1;
        if won {
            self.hands_won += 1;
        }
        if delta > self.biggest_win {
            self.biggest_win = delta;
        }
        if delta < self.biggest_loss {
            self.biggest_loss = delta;
        }
    }
}

fn build_session_summary(
    session: &SessionStats,
    players: &[Player],
    profiles: &TableProfiles,
    hero_seat: usize,
) -> Vec<String> {
    let cur_stack = players
        .get(hero_seat)
        .map(|p| p.stack)
        .unwrap_or(session.starting_stack);
    let net = cur_stack as i64 - session.starting_stack as i64;
    let win_rate = if session.hands_played == 0 {
        0.0
    } else {
        100.0 * session.hands_won as f64 / session.hands_played as f64
    };
    let mut out = Vec::new();
    out.push("── Session 总结 ──".to_string());
    out.push(format!(
        "  手数: {}    赢: {}    胜率: {:.1}%",
        session.hands_played, session.hands_won, win_rate
    ));
    let net_str = if net >= 0 {
        format!("+${net}")
    } else {
        format!("-${}", net.abs())
    };
    out.push(format!(
        "  起始 ${} → 当前 ${}    净盈亏: {}",
        session.starting_stack, cur_stack, net_str
    ));
    out.push(format!(
        "  最大单手赢: +${}    最大单手输: -${}",
        session.biggest_win,
        session.biggest_loss.abs()
    ));
    if let Some(profile) = profiles.profiles().get(hero_seat)
        && profile.hands_seen > 0
    {
        out.push(format!(
            "  你的 HUD: VP{:02} PF{:02} AF{:.1}    摊牌胜率: {:.1}%",
            (profile.vpip() * 100.0).round() as u32,
            (profile.pfr() * 100.0).round() as u32,
            profile.aggression_factor(),
            profile.showdown_win_rate() * 100.0,
        ));
    }
    out
}

/// 把动作映射到适合发声的事件（None = 不发声）。
fn action_to_event(action: Action, stage: Stage) -> Option<VoiceEvent> {
    match action {
        Action::Fold => Some(VoiceEvent::Folded),
        Action::AllIn => Some(VoiceEvent::AllIn),
        Action::Bet(_) | Action::Raise(_) if stage == Stage::Preflop => Some(VoiceEvent::OpenRaise),
        _ => None,
    }
}

/// 回放一手历史。按空格步进；q/Esc 中止；其他键继续自动播放。
fn replay_hand(
    h: &HandHistory,
    hero: usize,
    moods: &[Mood],
    no_anim: bool,
    layout: poker::config::Layout,
) -> std::io::Result<()> {
    use crossterm::event::{Event, KeyCode, poll, read};
    use std::time::Duration;

    let tilt_marks: Vec<bool> = moods.iter().map(|m| m.is_tilted()).collect();
    let header = vec![format!(
        "── 回放 (共 {} 帧, space 步进 / q 退出) ──",
        h.frames.len()
    )];
    for (idx, frame) in h.frames.iter().enumerate() {
        let is_last = idx + 1 == h.frames.len();
        let winners: &[usize] = if is_last { &h.winners } else { &[] };
        // 摊牌帧揭示所有手牌；其余按真人视角显示
        let reveal_all = is_last && h.winners.len() > 1;
        let mut frame_log = header.clone();
        let log_end = frame.log_len.min(h.logs.len());
        frame_log.extend_from_slice(&h.logs[..log_end]);
        render(
            &frame.state,
            RenderOptions {
                hero_seat: if reveal_all { None } else { Some(hero) },
                log: &frame_log,
                reveal_all,
                winners,
                tilt_marks: &tilt_marks,
                hud: None,
                layout,
            },
        )?;
        // 每帧默认 350ms 自动播放；按 space 立即步进；按 q/Esc 退出。
        let delay = if no_anim { 50 } else { 350 };
        let dur = Duration::from_millis(delay);
        if poll(dur)?
            && let Event::Key(k) = read()?
        {
            match k.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                _ => {}
            }
        }
    }
    pause(if no_anim { 0 } else { 600 });
    Ok(())
}
