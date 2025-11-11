use crate::world::{World, Patch};
use crate::operator::{Operator, OP_RLE};
use crate::pattern::Pattern;
use crate::stats::Stats;

/// Solver (Ratkaisija): toimija, jolla on oma sisäinen tila ja pieni "aivot"-muisti; 
/// se etsii, keksii ja soveltaa malleja.
pub struct Solver {
    pub processing_quota: u32,
    /// PatternBank: Solverin oppimien mallien muisti
    pub known_patterns: Vec<Pattern>,
    next_pattern_id: u32,
    /// Tilastot suorituskyvystä
    pub stats: Stats,
    /// Scheduler: päättää strategiset valinnat
    pub scheduler: crate::scheduler::Scheduler,
}

impl Solver {
    pub fn new(processing_quota: u32) -> Self {
        Solver {
            processing_quota,
            known_patterns: Vec::new(),
            next_pattern_id: 0,
            stats: Stats::new(),
            scheduler: crate::scheduler::Scheduler::new(),
        }
    }

    /// Pääsilmukka: Solver operoi Worldissä
    /// VAIHE 6: Quota-pohjainen silmukka, scheduler-ohjattu
    pub fn live(&mut self, world: &mut World, evaluator: &crate::evaluator::Evaluator) {
        // Nollaa syklin tilastot
        self.stats.reset_cycle();
        let mut current_quota = self.processing_quota; // Ota syklin budjetti

        // Alusta ikkuna jos se on tyhjä
        if world.window.start == world.window.end {
            let window_size = 4096.min(world.data.len());
            world.window = 0..window_size;
        }

        while current_quota > 0 {
            // Kysy schedulerilta mitä tehdä
            let pressure = world.data.len() as f64 / world.memory_limit as f64;
            let action = self.scheduler.decide_next_action(&self.stats, pressure);

            let quota_cost: u32;

            match action {
                crate::scheduler::Action::Exploit => {
                    let window_data = world.get_window_data();
                    if let Some((patch, pattern_id)) = self.exploit(window_data) {
                        // TÄRKEÄÄ: Muunna paikallinen patch globaaliksi
                        let global_patch = patch.clone_with_offset(world.window.start);

                        let cost_before = evaluator.calculate_total_cost(world);
                        let original_data = world.get_data_in_range(global_patch.range.clone());
                        
                        world.apply_patch(&global_patch);
                        let cost_after = evaluator.calculate_total_cost(world);
                        let gain = evaluator.calculate_gain(cost_before, cost_after);
                        
                        if gain > 0 {
                            quota_cost = 1;
                            self.stats.record_exploit(quota_cost, gain);
                            
                            // Päivitä mallin tilastot
                            if let Some(pattern) = self.known_patterns.iter_mut().find(|p| p.id == pattern_id) {
                                pattern.record_usage(gain);
                                println!("  ✓ Exploit onnistui (pattern #{}, säästö: {} tavua, käytetty {} kertaa)", 
                                         pattern_id, gain, pattern.usage_count);
                            }
                        } else {
                            world.rollback(&global_patch, original_data);
                            quota_cost = 1;
                            self.stats.record_exploit(quota_cost, 0);
                        }
                    } else {
                        quota_cost = 1; // Yritys maksoi
                        self.stats.record_exploit(quota_cost, 0);
                    }
                },

                crate::scheduler::Action::Explore => {
                    let window_data = world.get_window_data();
                    if let Some(patch) = self.explore(window_data) {
                        // TÄRKEÄÄ: Muunna paikallinen patch globaaliksi
                        let global_patch = patch.clone_with_offset(world.window.start);

                        let cost_before = evaluator.calculate_total_cost(world);
                        let original_data = world.get_data_in_range(global_patch.range.clone());
                        
                        // Tunnista operaattori patchistä
                        let operator = self.identify_operator(&global_patch);
                        
                        world.apply_patch(&global_patch);
                        let cost_after = evaluator.calculate_total_cost(world);
                        let gain = evaluator.calculate_gain(cost_before, cost_after);
                        
                        if gain <= 0 {
                            world.rollback(&global_patch, original_data);
                            quota_cost = 10;
                            self.stats.record_explore(quota_cost, 0);
                            println!("  ✗ Explore: Malli hylätty (tappio: {} tavua)", -gain);
                        } else {
                            quota_cost = 10;
                            self.stats.record_explore(quota_cost, gain);
                            
                            // OPI: Lisää PatternBankiin
                            let mut pattern = Pattern::new(self.next_pattern_id, operator);
                            pattern.record_usage(gain);
                            let pattern_id = self.next_pattern_id;
                            self.next_pattern_id += 1;
                            self.known_patterns.push(pattern);
                            println!("  ✓ Explore: Uusi malli #{} löydetty (säästö: {} tavua)", pattern_id, gain);
                            println!("  📚 PatternBank: {} mallia muistissa", self.known_patterns.len());
                        }
                    } else {
                        quota_cost = 10; // Yritys maksoi
                        self.stats.record_explore(quota_cost, 0);
                    }
                },

                crate::scheduler::Action::ShiftWindow => {
                    // Yksinkertainen heuristiikka: siirrä ikkunaa eteenpäin
                    // Tulevaisuudessa: fiksumpi "seek"
                    let new_start = (world.window.start + 3000) % world.data.len();
                    let window_size = 4096.min(world.data.len());
                    quota_cost = world.shift_window(new_start, window_size);
                    self.stats.record_seek(quota_cost);
                    println!("  🔍 ShiftWindow: Siirryttiin kohtaan {} (kustannus: {} quota)", new_start, quota_cost);
                },

                crate::scheduler::Action::MetaLearn => {
                    // Ei toteutettu vielä
                    quota_cost = 1;
                    println!("  🧠 MetaLearn: Ei vielä toteutettu");
                }
            }

            if current_quota >= quota_cost {
                current_quota -= quota_cost;
            } else {
                break; // Quota loppui tältä sykliltä
            }
        }

        self.update_cost_breakdown(world, evaluator);
    }

    /// Päivitä kustannuskomponentit statsiin
    fn update_cost_breakdown(&mut self, world: &World, evaluator: &crate::evaluator::Evaluator) {
        let (c_models, c_residual) = evaluator.calculate_cost_breakdown(world);
        self.stats.update_costs(c_models, c_residual);
    }

    /// Tunnista operaattori Patchista (yksinkertaistettu)
    fn identify_operator(&self, patch: &Patch) -> Operator {
        if patch.new_data.len() == 3 && patch.new_data[0] == OP_RLE {
            Operator::RunLength(patch.new_data[1], patch.new_data[2] as usize)
        } else {
            // Oletus: RunLength jos tuntematon
            Operator::RunLength(0, 0)
        }
    }

    /// Exploit: Käytä tunnettuja malleja
    /// Palauttaa: (Patch, pattern_id) jos löytyi sovellettava malli
    /// HUOM: Patch.range on paikallinen ikkunan suhteen!
    fn exploit(&self, data_slice: &[u8]) -> Option<(Patch, u32)> {
        // Käy läpi kaikki tunnetut mallit
        for pattern in &self.known_patterns {
            match &pattern.operator {
                Operator::RunLength(byte, min_count) => {
                    // Etsi tämän mallin mukaisia toistoja
                    if let Some(patch) = self.find_run_length(data_slice, *byte, *min_count) {
                        return Some((patch, pattern.id));
                    }
                }
            }
        }
        None
    }

    /// Explore: Etsi uusia malleja
    /// Tällä hetkellä: etsii RunLength-malleja (5+ samaa peräkkäistä tavua)
    /// HUOM: Patch.range on paikallinen ikkunan suhteen!
    fn explore(&self, data_slice: &[u8]) -> Option<Patch> {
        const MIN_RUN_LENGTH: usize = 5;
        self.find_any_run_length(data_slice, MIN_RUN_LENGTH)
    }

    /// Apufunktio: etsi mitä tahansa RunLength-mallia
    fn find_any_run_length(&self, data: &[u8], min_count: usize) -> Option<Patch> {
        if data.is_empty() {
            return None;
        }

        let mut i = 0;
        while i < data.len() {
            let byte = data[i];
            let mut count = 1;

            // Laske kuinka monta samaa tavua peräkkäin
            while i + count < data.len() && data[i + count] == byte {
                count += 1;
            }

            // Jos löytyi riittävän pitkä toisto, luo Patch
            if count >= min_count {
                let new_data = vec![OP_RLE, byte, count.min(255) as u8];
                return Some(Patch {
                    range: i..(i + count),
                    new_data,
                });
            }

            i += count;
        }

        None
    }

    /// Apufunktio: etsi tietyn tavun toistoa
    fn find_run_length(&self, data: &[u8], target_byte: u8, min_count: usize) -> Option<Patch> {
        if data.is_empty() {
            return None;
        }

        let mut i = 0;
        while i < data.len() {
            if data[i] == target_byte {
                let mut count = 1;
                while i + count < data.len() && data[i + count] == target_byte {
                    count += 1;
                }

                if count >= min_count {
                    let new_data = vec![OP_RLE, target_byte, count.min(255) as u8];
                    return Some(Patch {
                        range: i..(i + count),
                        new_data,
                    });
                }
                i += count;
            } else {
                i += 1;
            }
        }

        None
    }
}
