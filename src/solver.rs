use crate::world::{World, Patch};
use crate::operator::{Operator, OP_RLE, OP_LZ};
use crate::pattern::Pattern;
use crate::stats::Stats;

/// Solver (Ratkaisija): toimija, jolla on oma sisäinen tila ja pieni "aivot"-muisti; 
/// se etsii, keksii ja soveltaa malleja.
pub struct Solver {
    pub processing_quota: u32,
    /// PatternBank: Solverin oppimien mallien muisti
    pub known_patterns: Vec<Pattern>,
    next_pattern_id: u32,
    /// Kuinka monta mallia PatternBank voi pitää
    pub pattern_bank_capacity: usize,
    /// Tilastot suorituskyvystä
    pub stats: Stats,
    /// Scheduler: päättää strategiset valinnat
    pub scheduler: crate::scheduler::Scheduler,
}

impl Solver {
    const MIN_ACCEPT_GAIN: i32 = 12; // hylkää mikromallit joista nettohyöty pieni
    pub fn new(processing_quota: u32, pattern_bank_capacity: usize) -> Self {
        Solver {
            processing_quota,
            known_patterns: Vec::new(),
            next_pattern_id: 0,
            pattern_bank_capacity,
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
                    if let Some(patch) = self.explore(world) {
                        // TÄRKEÄÄ: Muunna paikallinen patch globaaliksi
                        let global_patch = patch.clone_with_offset(world.window.start);

                        let cost_before = evaluator.calculate_total_cost(world);
                        let original_data = world.get_data_in_range(global_patch.range.clone());
                        
                        // Tunnista operaattori patchistä
                        let operator = self.identify_operator(&global_patch);
                        
                        world.apply_patch(&global_patch);
                        let cost_after = evaluator.calculate_total_cost(world);
                        let gain = evaluator.calculate_gain(cost_before, cost_after);
                        
                        if gain < Solver::MIN_ACCEPT_GAIN {
                            world.rollback(&global_patch, original_data);
                            quota_cost = 10;
                            self.stats.record_explore(quota_cost, 0);
                            println!("  ✗ Explore: Malli hylätty (liian pieni hyöty: {} tavua)", gain);
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
                            println!("  📚 PatternBank: {}/{} mallia muistissa", self.known_patterns.len(), self.pattern_bank_capacity);
                            // Muista unohtaa, jos täynnä
                            self.forget_if_needed();
                        }
                    } else {
                        quota_cost = 10; // Yritys maksoi
                        self.stats.record_explore(quota_cost, 0);
                    }
                },

                crate::scheduler::Action::ShiftWindow => {
                    // Siirrä ikkunaa, mutta älä kuluta koko quotaa
                    let data_len = world.data.len();
                    if data_len == 0 || data_len <= 4096 {
                        // Ei dataa tai kaikki mahtuu yhteen ikkunaan — ei kannata siirtää
                        quota_cost = 1;
                        self.stats.record_seek(quota_cost);
                        println!("  🔍 ShiftWindow: Ei tarvetta siirtää ikkunaa (kustannus: {})", quota_cost);
                    } else {
                        // Siirrä ikkunaa satunnaiseen paikkaan
                        use rand::Rng;
                        let mut rng = rand::thread_rng();
                        let new_start = rng.gen_range(0..data_len);
                        let window_size = 4096.min(data_len - new_start);
                        // Kiinteä kustannus välttää liian kalliita siirtoja
                        quota_cost = 5; 
                        world.window = new_start..(new_start + window_size);
                        self.stats.record_seek(quota_cost);
                        println!("  🔍 ShiftWindow: Siirryttiin kohtaan {} (kustannus: {} quota)", new_start, quota_cost);
                    }
                },

                crate::scheduler::Action::MetaLearn => {
                    // Meta-oppiminen on kallista
                    quota_cost = 100;
                    if current_quota >= quota_cost {
                        println!("  🧠 MetaLearn: Käynnistetään meta-oppiminen...");
                        self.meta_learn();
                        self.stats.record_meta(quota_cost, 0);
                    } else {
                        println!("  🧠 MetaLearn: Ei tarpeeksi quotaa (tarvitaan {})", quota_cost);
                    }
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

    /// Etsi malleja malleista (hyvin yksinkertainen runko)
    fn meta_learn(&mut self) {
        println!("  🧠 MetaLearn: Tutkitaan PatternBankia...");

        // Laske, kuinka monta RunLength-mallia meillä on
        let rle_count = self.known_patterns.iter()
            .filter(|p| matches!(p.operator, Operator::RunLength(_, _)))
            .count();

        if rle_count > 10 {
            println!("  🧠 Oivallus: RunLength-malleja paljon ({} kpl). Priorisoi RunLength-tutkimusta.", rle_count);
            // Esimerkki: säädä Schedulerin painotusta siten, että eksplorointi RunLength-tyyppisiin paikkoihin kasvaa
            self.scheduler.increase_exploit_bias(0.1);
            println!("  🧠 MetaLearn: Schedulerin exploit-biasia kasvatettu.");
        } else {
            println!("  🧠 MetaLearn: Ei löydetty vahvaa abstraktiota (RLE count={}).", rle_count);
        }
    }

    /// Poista huonoin malli, jos PatternBank on täynnä
    fn forget_if_needed(&mut self) {
        if self.known_patterns.len() > self.pattern_bank_capacity {
            use std::time::SystemTime;
            let now = SystemTime::now();

            // Rankkaa mallit yhdistetyllä scorella: score = bytes_saved*B + usage_count*U - age_sec*A
            let bytes_weight = 1.0;
            let usage_weight = 10.0;
            let age_penalty = 0.1; // per second

            let mut worst_index = 0usize;
            let mut worst_score = std::f64::INFINITY;

            for (i, p) in self.known_patterns.iter().enumerate() {
                let age = now.duration_since(p.last_used).unwrap_or_default().as_secs() as f64;
                let score = (p.total_bytes_saved as f64) * bytes_weight + (p.usage_count as f64) * usage_weight - age * age_penalty;
                if score < worst_score {
                    worst_score = score;
                    worst_index = i;
                }
            }

            let removed = self.known_patterns.remove(worst_index);
            println!("  🗑️ Forget: PatternBank täynnä. Poistettiin malli #{}, score {:.2}, säästö {} tavua, käyttöjä {}.",
                     removed.id, worst_score, removed.total_bytes_saved, removed.usage_count);
        }
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
        } else if patch.new_data.len() == 4 && patch.new_data[0] == OP_LZ {
            let dist = (patch.new_data[1] as usize) | ((patch.new_data[2] as usize) << 8);
            let len = patch.new_data[3] as usize;
            Operator::BackRef(dist, len)
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
                Operator::BackRef(distance, length) => {
                    // Hyödynnä LZ-tyylistä viittausta jos sekä lähde- että kohdeikkuna ovat
                    // näkyvissä tässä ikkunassa.
                    let dist = *distance;
                    let len = (*length).min(255);
                    if data_slice.len() >= len && dist > 0 {
                        // Skannaa kaikki mahdolliset kohde-alkiot ikkunassa
                        let max_start = data_slice.len().saturating_sub(len);
                        for i in 0..=max_start {
                            // Lähdealueen täytyy osua ikkunaan
                            if i < dist { continue; }
                            let src = i - dist;
                            // Varmista, että myös lähde-alue mahtuu viipaleeseen
                            if src + len > data_slice.len() { continue; }
                            if &data_slice[i..i+len] == &data_slice[src..src+len] {
                                let new_data = vec![
                                    OP_LZ,
                                    (dist & 0xFF) as u8,
                                    ((dist >> 8) & 0xFF) as u8,
                                    len as u8,
                                ];
                                let patch = Patch { range: i..(i + len), new_data };
                                return Some((patch, pattern.id));
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Explore: yritä ensin BackRef (LZ), sitten run-length fallback
    fn explore(&self, world: &World) -> Option<Patch> {
        if let Some(p) = self.find_backref(world, 5, 16384) { // min len 5, max distance 16KB
            return Some(p);
        }
        let slice = world.get_window_data();
        self.find_any_run_length(slice, 3)
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

    /// LZ-tyylinen backreference etsimällä ikkunan alun (ws) pisin match edeltävästä datasta
    fn find_backref(&self, world: &World, min_len: usize, max_distance: usize) -> Option<Patch> {
        let data = &world.data;
        let ws = world.window.start;
        let we = world.window.end.min(data.len());
        if we - ws < min_len + 1 { return None; }

        let mut best_target = 0usize;
        let mut best_src = 0usize;
        let mut best_len = 0usize;

        // Tutki muutama kohde ikkunassa (alku, 1/4, 1/2, 3/4)
        let span = we - ws;
        let candidates = [ws + 1, ws + span/4, ws + span/2, ws + (3*span/4)].into_iter()
            .filter(|&t| t > ws && t + min_len <= we);

        for target in candidates {
            let max_dist = (target - ws).min(max_distance);
            let max_len_possible = (we - target).min(255);
            let search_start = target.saturating_sub(max_dist);
            for src in search_start..target {
                let mut l = 0usize;
                while l < max_len_possible && data[src + l] == data[target + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_src = src;
                    best_target = target;
                    if l == max_len_possible { break; }
                }
            }
        }

        if best_len >= min_len {
            let distance = best_target - best_src;
            let enc_len = best_len.min(255);
            let new_data = vec![OP_LZ, (distance & 0xFF) as u8, ((distance >> 8) & 0xFF) as u8, enc_len as u8];
            // Paikallinen range offset = best_target - ws
            let local_start = best_target - ws;
            return Some(Patch { range: local_start..(local_start + enc_len), new_data });
        }
        None
    }
}
