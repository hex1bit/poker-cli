use poker::bot::Personality;
use poker::bot::decision::decide;
use poker::game::action::Action;
use poker::game::betting::{advance_street, apply_action, round_closed, start_hand};
use poker::game::showdown::settle;
use poker::game::state::{Player, Stage};
use rand::SeedableRng;

fn main() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);
    let mut total_actions = 0u64;
    let mut voluntary_jam = 0u64;     // 主动 jam (raise to all-in)
    let mut forced_call_jam = 0u64;   // 跟注 all-in (to_call >= stack)
    let mut deep_voluntary = 0u64;    // 主动 jam @ 深筹 (>30bb)
    let mut med_voluntary = 0u64;     // 主动 jam @ 中筹 (15-30bb)
    let mut short_voluntary = 0u64;   // 主动 jam @ 短筹 (<=15bb)
    let personas = Personality::PRESETS;
    for hand in 0..500 {
        let players: Vec<Player> = (0..6)
            .map(|i| Player::new(format!("B{i}"), 1500))
            .collect();
        let mut s = start_hand(players, hand % 6, 5, 10, &mut rng);
        let mut steps = 0;
        while s.stage != Stage::Complete && s.stage != Stage::Showdown && steps < 500 {
            if let Some(seat) = s.to_act {
                let p = personas[seat % personas.len()];
                let stack = s.players[seat].stack;
                let to_call = s.to_call_for(seat);
                let pot = s.total_pot().max(s.bb) as f64;
                let spr = stack as f64 / pot;
                let a = decide(&s, seat, p, &mut rng);
                if matches!(a, Action::AllIn) {
                    if to_call >= stack {
                        forced_call_jam += 1;
                    } else {
                        voluntary_jam += 1;
                        // 按 SPR 而非 raw stack 分桶（poker 上更有意义）
                        if spr >= 4.0 {
                            deep_voluntary += 1;
                        } else if spr >= 1.5 {
                            med_voluntary += 1;
                        } else {
                            short_voluntary += 1;
                        }
                    }
                }
                total_actions += 1;
                apply_action(&mut s, seat, a).expect("legal");
            } else if round_closed(&s) {
                advance_street(&mut s);
            }
            steps += 1;
        }
        if s.stage == Stage::Showdown {
            settle(&mut s);
        }
    }
    let total_jam = voluntary_jam + forced_call_jam;
    println!(
        "hands=500 actions={total_actions}\n  total all-in    : {} ({:.1}%)\n  forced (call jam): {} ({:.1}%)\n  voluntary (raise → jam): {} ({:.1}%)\n    @ deep   (>30bb): {} ({:.1}% of voluntary)\n    @ medium (15-30): {} ({:.1}% of voluntary)\n    @ short  (<=15bb): {} ({:.1}% of voluntary)",
        total_jam, 100.0 * total_jam as f64 / total_actions as f64,
        forced_call_jam, 100.0 * forced_call_jam as f64 / total_actions as f64,
        voluntary_jam,   100.0 * voluntary_jam   as f64 / total_actions as f64,
        deep_voluntary,  100.0 * deep_voluntary  as f64 / voluntary_jam.max(1) as f64,
        med_voluntary,   100.0 * med_voluntary   as f64 / voluntary_jam.max(1) as f64,
        short_voluntary, 100.0 * short_voluntary as f64 / voluntary_jam.max(1) as f64,
    );
}
