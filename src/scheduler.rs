// src/scheduler.rs
use crate::stats::Stats;
use rand::Rng;

#[derive(Debug, Clone, Copy)]
pub enum Action {
    Exploit,
    Explore,
    ShiftWindow,
    MetaLearn, // Tulevaisuutta varten
}

pub struct Scheduler {
    /// Kynnys/exploit-bias parametrit, muokattavissa meta-learnillä
    pub exploit_threshold: f64,
    pub exploit_bias_prob: f64,
    pub explore_vs_shift_prob: f64,
    pub meta_prob: f64,
}

impl Scheduler {
    pub fn new() -> Self { 
        Scheduler { 
            exploit_threshold: 1.0,
            exploit_bias_prob: 0.7,
            explore_vs_shift_prob: 0.9, // Priorisoi explore/exploit yli shift
            meta_prob: 0.01,
        } 
    }

    /// Päättää seuraavan toimenpiteen tilastojen perusteella
    pub fn decide_next_action(&self, stats: &Stats, world_pressure: f64) -> Action {
        let mut rng = rand::thread_rng();

        // Satunnaisesti meta-oppiminen  
        let roll = rng.gen_range(0.0..1.0);
        if roll < self.meta_prob {
            return Action::MetaLearn;
        }

        // Jos exploit on tuottavaa, käytä sitä usein
        if stats.gain_per_quota_exploit > 0.0 && stats.gain_per_quota_exploit >= stats.gain_per_quota_explore {
            if rng.gen_range(0.0..1.0) < self.exploit_bias_prob {
                return Action::Exploit;
            }
        }

        // Jos explore on tuottavaa, käytä sitä
        if stats.gain_per_quota_explore > 0.0 {
            if rng.gen_range(0.0..1.0) < 0.7 {
                return Action::Explore;
            }
        }
        // Fallback: päätä explore vs. window shift
        // Kasvata ikkunansiirron todennäköisyyttä kun paine kasvaa
        let shift_prob = (1.0 - self.explore_vs_shift_prob) + (world_pressure * 0.2);
        if rng.gen_range(0.0..1.0) < shift_prob.min(0.6) {
            Action::ShiftWindow
        } else if rng.gen_range(0.0..1.0) < 0.3 {
            // Siemennä myös exploit-tilastoja
            Action::Exploit
        } else {
            Action::Explore
        }
    }

    /// Kasvata exploit-biasia (meta-operaatiosta hyödyntämiseen)
    pub fn increase_exploit_bias(&mut self, delta: f64) {
        self.exploit_bias_prob = (self.exploit_bias_prob + delta).min(1.0);
    }

    /// Aseta eri scheduler-parametrit suoraan
    pub fn set_params(&mut self, exploit_threshold: f64, exploit_bias_prob: f64, explore_vs_shift_prob: f64, meta_prob: f64) {
        self.exploit_threshold = exploit_threshold;
        self.exploit_bias_prob = exploit_bias_prob;
        self.explore_vs_shift_prob = explore_vs_shift_prob;
        self.meta_prob = meta_prob;
    }
}
