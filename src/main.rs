//! Texas Hold'em CLI 主入口。

use std::io::ErrorKind;

use clap::Parser;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;

use poker::bot::{Mood, VoiceEvent, decision::decide_with_mood, names::sample_table_names, pick_line};
use poker::config::Cli;
use poker::game::action::Action;
use poker::game::betting::{advance_street, apply_action, round_closed, start_hand};
use poker::game::history::HandHistory;
use poker::game::showdown::settle;
use poker::game::state::{Player, PlayerStatus, Stage};
use poker::ui::anim::{animate_deal_community, pause};
use poker::ui::input::prompt_action;
use poker::ui::render::{ContinueAction, enter_screen, leave_screen, render, render_showdown, wait_for_continue};

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
    let personalities = cli.resolve_personalities();
    let n_bots = personalities.len();
    if n_bots < 1 || n_bots > 6 {
        return Err(std::io::Error::new(
            ErrorKind::InvalidInput,
            "bots must be 1..=6",
        ));
    }

    // 座位 0 = 真人，座位 1..=N = bots。
    let mut players: Vec<Player> = Vec::with_capacity(n_bots + 1);
    players.push(Player::new(cli.name.clone(), cli.stack));
    let mut rng = StdRng::from_entropy();
    let bot_names = sample_table_names(&personalities, &mut rng);
    for (i, _p) in personalities.iter().enumerate() {
        players.push(Player::new(bot_names[i].clone(), cli.stack));
    }
    let hero_seat = 0usize;
    let mut button = personalities.len(); // 第一手按钮位放在最右 bot 上，让真人是 SB（头对头时为 button）
    // 每个座位一份 mood（hero 也有，但不会读取）
    let mut moods: Vec<Mood> = (0..=n_bots).map(|_| Mood::new()).collect();
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
            .map(|(i, p)| format!("{} ({})", bot_names[i], p.label))
            .collect::<Vec<_>>()
            .join(", ")
    ));

    enter_screen()?;

    let mut hand_idx: u32 = 0;
    let result = loop {
        // 检查只剩一人：游戏结束
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
        log.push(format!("── Hand #{} (button = {}) ──", hand_idx + 1, players[button].name));
        log.push(format!(
            "Blinds posted. Pot ${}.",
            state.total_pot()
        ));
        let tilt_marks: Vec<bool> = moods.iter().map(|m| m.is_tilted()).collect();
        render(&state, Some(hero_seat), &log, false, &[], &tilt_marks)?;
        let mut hand_history = HandHistory::new();
        hand_history.push(&state, log.len());

        // 进行下注/发牌循环
        loop {
            // 处理动作或推进街
            if let Some(seat) = state.to_act {
                let action = if seat == hero_seat {
                    match prompt_action(&state, seat) {
                        Ok(a) => a,
                        Err(e) if e.kind() == ErrorKind::Interrupted => {
                            break_game_with_msg(&mut log, "User quit.");
                            return finish(log);
                        }
                        Err(e) => return Err(e),
                    }
                } else {
                    let p = personalities[seat - 1];
                    decide_with_mood(&state, seat, p, &moods[seat], &mut rng)
                };

                match apply_action(&mut state, seat, action) {
                    Ok(()) => {
                        log.push(format!("{} {}", state.players[seat].name, action));
                        // 触发台词（仅 bot）
                        if seat != hero_seat && !cli.quiet {
                            let p = personalities[seat - 1];
                            let ev = action_to_event(action, state.stage);
                            if let Some(ev) = ev {
                                if let Some(line) = pick_line(&p, ev, &mut rng) {
                                    log.push(format!(
                                        "{}：「{}」",
                                        state.players[seat].name, line
                                    ));
                                }
                            }
                        }
                        // Show one card：bot 弃牌 → 按 show_freq 概率秀一张牌
                        if action == Action::Fold && seat != hero_seat {
                            let p = personalities[seat - 1];
                            if rng.r#gen::<f64>() < p.show_freq {
                                if let Some(hole) = state.players[seat].hole {
                                    let pick = if rng.r#gen::<f64>() < 0.5 { hole[0] } else { hole[1] };
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
                render(&state, Some(hero_seat), &log, false, &[], &tilt_marks)?;
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
                        log.push(format!("── {:?} ──", state.stage));
                        let tilt_marks: Vec<bool> = moods.iter().map(|m| m.is_tilted()).collect();
                        let new_cards = match prev_stage {
                            Stage::Preflop => 3, // flop
                            _ => 1,              // turn / river
                        };
                        let delay = if cli.no_anim { 0 } else { 220 };
                        animate_deal_community(
                            &state,
                            Some(hero_seat),
                            &log,
                            &tilt_marks,
                            new_cards,
                            delay,
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
            render(&state, Some(hero_seat), &log, false, &winners, &tilt_marks)?;
        } else {
            // 谁拿到最多
            let mut order: Vec<(usize, u64)> = state
                .players
                .iter()
                .enumerate()
                .map(|(i, p)| (i, p.committed_total))
                .collect();
            order.sort_by_key(|&(_, c)| std::cmp::Reverse(c));
            log.push("Showdown:".to_string());
            for i in 0..state.players.len() {
                if state.players[i].status != PlayerStatus::Folded {
                    if let Some(h) = state.players[i].hole {
                        log.push(format!(
                            "  {}: [{}] [{}]",
                            state.players[i].name, h[0], h[1]
                        ));
                    }
                }
            }
            let tilt_marks: Vec<bool> = moods.iter().map(|m| m.is_tilted()).collect();
            render_showdown(&state, &log, &winners, &tilt_marks)?;
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
            let now_tilted = moods[i].is_tilted();
            if !was_tilted && now_tilted && !cli.quiet {
                if let Some(line) = pick_line(&p, VoiceEvent::TiltOn, &mut rng) {
                    log.push(format!("{}：「{}」", state.players[i].name, line));
                }
            }
        }
        // 保存最终一帧 + 赢家
        hand_history.push(&state, log.len());
        hand_history.winners = winners.clone();
        hand_history.final_log_len = log.len();
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
                        replay_hand(last, hero_seat, &moods, cli.no_anim)?;
                        // 回放结束后重绘当前桌面，再次提示
                        let tilt_marks: Vec<bool> =
                            moods.iter().map(|m| m.is_tilted()).collect();
                        render(&state, Some(hero_seat), &log, true, &winners, &tilt_marks)?;
                    }
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {
                    break_game_with_msg(&mut log, "User quit.");
                    return finish(log);
                }
                Err(e) => return Err(e),
            }
        }

        // 把筹码写回主玩家列表
        players = state.players;

        hand_idx += 1;
    };

    leave_screen()?;
    for l in log {
        println!("{l}");
    }
    result
}

fn finish(log: Vec<String>) -> std::io::Result<()> {
    leave_screen()?;
    for l in log {
        println!("{l}");
    }
    Ok(())
}

fn break_game_with_msg(log: &mut Vec<String>, msg: &str) {
    log.push(msg.to_string());
}

/// 按钮位移到下一位 stack > 0 的玩家。
fn advance_button(players: &[Player], current: usize) -> usize {
    let n = players.len();
    for step in 1..=n {
        let i = (current + step) % n;
        if players[i].stack > 0 {
            return i;
        }
    }
    current
}

/// 把动作映射到适合发声的事件（None = 不发声）。
fn action_to_event(action: Action, stage: Stage) -> Option<VoiceEvent> {
    match action {
        Action::Fold => Some(VoiceEvent::Folded),
        Action::AllIn => Some(VoiceEvent::AllIn),
        Action::Bet(_) | Action::Raise(_) if stage == Stage::Preflop => {
            Some(VoiceEvent::OpenRaise)
        }
        _ => None,
    }
}

/// 回放一手历史。按空格步进；q/Esc 中止；其他键继续自动播放。
fn replay_hand(
    h: &HandHistory,
    hero: usize,
    moods: &[Mood],
    no_anim: bool,
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
        render(
            &frame.state,
            if reveal_all { None } else { Some(hero) },
            &header,
            reveal_all,
            winners,
            &tilt_marks,
        )?;
        // 每帧默认 350ms 自动播放；按 space 立即步进；按 q/Esc 退出。
        let delay = if no_anim { 50 } else { 350 };
        let dur = Duration::from_millis(delay);
        if poll(dur)? {
            if let Event::Key(k) = read()? {
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    _ => {}
                }
            }
        }
    }
    pause(if no_anim { 0 } else { 600 });
    Ok(())
}
