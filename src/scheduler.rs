// src/scheduler.rs
use crate::stats::Stats;

#[derive(Debug, Clone, Copy)]
pub enum Action {
    Exploit,
    Explore,
    ShiftWindow,
    MetaLearn, // Tulevaisuutta varten
}

pub struct Scheduler {}

impl Scheduler {
    pub fn new() -> Self { 
        Scheduler {} 
    }

    /// Päättää seuraavan toimenpiteen tilastojen perusteella
    /// TÄMÄ ON PAIKKA, JOSSA ÄLYKKYYS SYNTYY!
    pub fn decide_next_action(&self, stats: &Stats, world_pressure: f64) -> Action {
        // VAIHE 6.1 (Yksinkertainen malli):
        // Jos exploit on tuottavaa, tee sitä.
        if stats.gain_per_quota_exploit > 10.0 {
            return Action::Exploit;
        }

        // Jos paine on kova (esim. > 80% täynnä), älä tutki, vaan siirrä ikkunaa
        // ja yritä löytää nopeita voittoja
        if world_pressure > 0.8 && stats.gain_per_quota_exploit > 0.0 {
             // Yritä löytää toinen paikka käyttää vanhaa mallia
            return Action::ShiftWindow; 
        }

        // Oletus: tutki uutta
        Action::Explore
    }
}
