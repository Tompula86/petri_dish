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
    #[allow(dead_code)]
    pub exploit_threshold: f64,
    pub exploit_bias_prob: f64,
    pub meta_prob: f64,
    pub risk_budget_ratio: f64,
    pub shift_base_prob: f64,
}

impl Scheduler {
    pub fn new() -> Self { 
        Scheduler { 
            exploit_threshold: 1.0,
            exploit_bias_prob: 0.7,
            meta_prob: 0.01,
            risk_budget_ratio: 0.1,
            shift_base_prob: 0.2,
        } 
    }

    /// Päättää seuraavan toimenpiteen tilastojen perusteella
    pub fn decide_next_action(
        &self,
        stats: &Stats,
        world_pressure: f64,
        remaining_quota: u32,
        cycle_quota: u32,
    ) -> Action {
        let mut rng = rand::thread_rng();

        // Riskibudjetti: pakota pieni osa quotasta explorointiin joka kierroksella
        let required_explore_quota = (self.risk_budget_ratio * cycle_quota as f64).ceil() as u32;
        if stats.quota_spent_explore < required_explore_quota && remaining_quota >= 10 {
            return Action::Explore;
        }

        // Satunnaisesti meta-oppiminen  
        let roll = rng.gen_range(0.0..1.0);
        if roll < self.meta_prob {
            return Action::MetaLearn;
        }

        let stagnating = stats.total_gain() <= 0;
        let shift_pressure = (self.shift_base_prob + world_pressure * 0.3).clamp(0.05, 0.9);
        if stagnating || (remaining_quota >= 5 && rng.gen_range(0.0..1.0) < shift_pressure) {
            return Action::ShiftWindow;
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
        // Fallback: päätä explore vs. exploit paineen perusteella
        let explore_bias = (0.35 + world_pressure * 0.4).clamp(0.2, 0.85);
        if rng.gen_range(0.0..1.0) < explore_bias {
            Action::Explore
        } else {
            Action::Exploit
        }
    }

    /// Kasvata exploit-biasia (meta-operaatiosta hyödyntämiseen)
    #[allow(dead_code)]
    pub fn increase_exploit_bias(&mut self, delta: f64) {
        self.exploit_bias_prob = (self.exploit_bias_prob + delta).min(1.0);
    }

    /// Aseta eri scheduler-parametrit suoraan
    #[allow(dead_code)]
    pub fn set_params(&mut self, exploit_threshold: f64, exploit_bias_prob: f64, meta_prob: f64) {
        self.exploit_threshold = exploit_threshold;
        self.exploit_bias_prob = exploit_bias_prob;
        self.meta_prob = meta_prob;
    }
}
