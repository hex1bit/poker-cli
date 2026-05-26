//! 对手画像统计。
//!
//! 这里不尝试做复杂范围推断，只记录可稳定从行动流观察到的 VPIP / PFR /
//! aggression / showdown 数据，供 bot 做轻量调参。

use crate::game::action::Action;
use crate::game::state::{HandState, PlayerStatus, Stage};

#[derive(Debug, Clone, Default)]
pub struct OpponentProfile {
    pub hands_seen: u32,
    pub vpip_count: u32,
    pub pfr_count: u32,
    pub aggression_actions: u32,
    pub passive_actions: u32,
    pub showdown_count: u32,
    pub won_showdown_count: u32,
}

impl OpponentProfile {
    pub fn vpip(&self) -> f64 {
        ratio(self.vpip_count, self.hands_seen)
    }

    pub fn pfr(&self) -> f64 {
        ratio(self.pfr_count, self.hands_seen)
    }

    pub fn aggression_factor(&self) -> f64 {
        if self.passive_actions == 0 {
            return self.aggression_actions as f64;
        }
        self.aggression_actions as f64 / self.passive_actions as f64
    }

    pub fn showdown_win_rate(&self) -> f64 {
        ratio(self.won_showdown_count, self.showdown_count)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TableRead {
    pub avg_vpip: f64,
    pub avg_pfr: f64,
    pub avg_aggression: f64,
    pub samples: usize,
}

#[derive(Debug, Clone, Default)]
struct HandFlags {
    saw_hand: bool,
    vpip: bool,
    pfr: bool,
}

#[derive(Debug, Clone)]
pub struct TableProfiles {
    profiles: Vec<OpponentProfile>,
    current: Vec<HandFlags>,
}

impl TableProfiles {
    pub fn new(seats: usize) -> Self {
        Self {
            profiles: vec![OpponentProfile::default(); seats],
            current: vec![HandFlags::default(); seats],
        }
    }

    pub fn start_hand(&mut self, state: &HandState) {
        self.ensure_len(state.players.len());
        for (flags, p) in self.current.iter_mut().zip(&state.players) {
            *flags = HandFlags {
                saw_hand: p.status != PlayerStatus::SitOut,
                vpip: false,
                pfr: false,
            };
        }
    }

    pub fn observe_action(&mut self, stage: Stage, seat: usize, action: Action) {
        self.ensure_len(seat + 1);
        match action {
            Action::Bet(_) | Action::Raise(_) | Action::AllIn => {
                self.profiles[seat].aggression_actions += 1;
                if stage == Stage::Preflop {
                    self.current[seat].vpip = true;
                    self.current[seat].pfr = true;
                }
            }
            Action::Call => {
                self.profiles[seat].passive_actions += 1;
                if stage == Stage::Preflop {
                    self.current[seat].vpip = true;
                }
            }
            Action::Check => {
                self.profiles[seat].passive_actions += 1;
            }
            Action::Fold => {}
        }
    }

    pub fn finish_hand(&mut self, state: &HandState, winners: &[usize]) {
        self.ensure_len(state.players.len());
        for (seat, flags) in self.current.iter().enumerate().take(state.players.len()) {
            if !flags.saw_hand {
                continue;
            }
            self.profiles[seat].hands_seen += 1;
            if flags.vpip {
                self.profiles[seat].vpip_count += 1;
            }
            if flags.pfr {
                self.profiles[seat].pfr_count += 1;
            }
            if state.community.len() == 5 && state.players[seat].status != PlayerStatus::Folded {
                self.profiles[seat].showdown_count += 1;
                if winners.contains(&seat) {
                    self.profiles[seat].won_showdown_count += 1;
                }
            }
        }
    }

    pub fn table_read_for(&self, hero: usize, state: &HandState) -> TableRead {
        let mut samples = 0usize;
        let mut vpip = 0.0;
        let mut pfr = 0.0;
        let mut aggression = 0.0;
        for (seat, p) in state.players.iter().enumerate() {
            if seat == hero || !p.is_in_hand() {
                continue;
            }
            let Some(profile) = self.profiles.get(seat) else {
                continue;
            };
            if profile.hands_seen == 0 {
                continue;
            }
            samples += 1;
            vpip += profile.vpip();
            pfr += profile.pfr();
            aggression += profile.aggression_factor();
        }
        if samples == 0 {
            return TableRead::default();
        }
        TableRead {
            avg_vpip: vpip / samples as f64,
            avg_pfr: pfr / samples as f64,
            avg_aggression: aggression / samples as f64,
            samples,
        }
    }

    pub fn profiles(&self) -> &[OpponentProfile] {
        &self.profiles
    }

    pub fn reset_seat(&mut self, seat: usize) {
        self.ensure_len(seat + 1);
        self.profiles[seat] = OpponentProfile::default();
        self.current[seat] = HandFlags::default();
    }

    pub fn hud_lines(&self, seats: usize) -> Vec<String> {
        (0..seats)
            .map(|seat| {
                let Some(profile) = self.profiles.get(seat) else {
                    return "VP-- PF-- AF--".to_string();
                };
                if profile.hands_seen == 0 {
                    return "VP-- PF-- AF--".to_string();
                }
                format!(
                    "VP{:02} PF{:02} AF{:>3.1}",
                    (profile.vpip() * 100.0).round() as u32,
                    (profile.pfr() * 100.0).round() as u32,
                    profile.aggression_factor()
                )
            })
            .collect()
    }

    fn ensure_len(&mut self, seats: usize) {
        if self.profiles.len() < seats {
            self.profiles.resize_with(seats, OpponentProfile::default);
        }
        if self.current.len() < seats {
            self.current.resize_with(seats, HandFlags::default);
        }
    }
}

fn ratio(num: u32, den: u32) -> f64 {
    if den == 0 {
        0.0
    } else {
        num as f64 / den as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::betting::start_hand;
    use crate::game::state::Player;
    use rand::SeedableRng;

    #[test]
    fn tracks_vpip_and_pfr() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let state = start_hand(
            vec![
                Player::new("A", 100),
                Player::new("B", 100),
                Player::new("C", 100),
            ],
            0,
            5,
            10,
            &mut rng,
        );
        let mut profiles = TableProfiles::new(3);
        profiles.start_hand(&state);
        profiles.observe_action(Stage::Preflop, 0, Action::Raise(30));
        profiles.observe_action(Stage::Preflop, 1, Action::Call);
        profiles.finish_hand(&state, &[0]);

        assert_eq!(profiles.profiles()[0].vpip(), 1.0);
        assert_eq!(profiles.profiles()[0].pfr(), 1.0);
        assert_eq!(profiles.profiles()[1].vpip(), 1.0);
        assert_eq!(profiles.profiles()[1].pfr(), 0.0);
        assert_eq!(profiles.hud_lines(2)[0], "VP100 PF100 AF1.0");
    }
}
