use crate::world::{World, Patch};
use crate::operator::{Operator, OP_DELTA, OP_LZ, OP_RLE, OP_XOR};
use crate::pattern::Pattern;
use crate::stats::Stats;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

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
    /// Sanakirja: word_id -> word bytes; käytetään Dictionary-operaattoreissa
    dictionary: HashMap<u32, Vec<u8>>,
    /// Käänteinen sanakirja: word bytes -> word_id; nopeaan hakuun
    word_to_id: HashMap<Vec<u8>, u32>,
    /// Seuraava vapaa word_id
    next_word_id: u32,
}

impl Solver {
    const MIN_ACCEPT_GAIN: i32 = 12; // hylkää mikromallit joista nettohyöty pieni
    const PATTERN_BANK_FILE: &'static str = "pattern_bank.json";

    pub fn new(processing_quota: u32, pattern_bank_capacity: usize) -> Self {
        Solver {
            processing_quota,
            known_patterns: Vec::new(),
            next_pattern_id: 0,
            pattern_bank_capacity,
            stats: Stats::new(),
            scheduler: crate::scheduler::Scheduler::new(),
            dictionary: HashMap::new(),
            word_to_id: HashMap::new(),
            next_word_id: 0,
        }
    }

    /// Lataa Solver aiemmin tallennetuista malleista, tai luo uuden jos tiedostoa ei ole
    pub fn load_or_new(processing_quota: u32, pattern_bank_capacity: usize) -> Self {
        if let Ok(contents) = std::fs::read_to_string(Self::PATTERN_BANK_FILE) {
            if let Ok(patterns) = serde_json::from_str::<Vec<Pattern>>(&contents) {
                let max_id = patterns.iter().map(|p| p.id).max().unwrap_or(0);
                println!("  ♻️  Ladattiin {} mallia tiedostosta {}", patterns.len(), Self::PATTERN_BANK_FILE);
                let mut solver = Solver {
                    processing_quota,
                    known_patterns: patterns,
                    next_pattern_id: max_id + 1,
                    pattern_bank_capacity,
                    stats: Stats::new(),
                    scheduler: crate::scheduler::Scheduler::new(),
                    dictionary: HashMap::new(),
                    word_to_id: HashMap::new(),
                    next_word_id: 0,
                };
                // Rakennetaan sanakirja ladatuista Dictionary-malleista
                solver.rebuild_dictionary();
                return solver;
            }
        }
        println!("  🆕 Luodaan uusi Solver (ei aiempia malleja)");
        Self::new(processing_quota, pattern_bank_capacity)
    }

    /// Tallenna tunnetut mallit levylle
    pub fn save_patterns(&self) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(&self.known_patterns)?;
        std::fs::write(Self::PATTERN_BANK_FILE, json)?;
        println!("  💾 Tallennettu {} mallia tiedostoon {}", self.known_patterns.len(), Self::PATTERN_BANK_FILE);
        Ok(())
    }

    /// Pääsilmukka: Solver operoi Worldissä
    /// VAIHE 6: Quota-pohjainen silmukka, scheduler-ohjattu
    pub fn live(&mut self, world: &mut World, evaluator: &crate::evaluator::Evaluator) {
        // Nollaa syklin tilastot
        self.stats.reset_cycle();
        self.decay_recent_gains();
        let mut current_quota = self.processing_quota; // Ota syklin budjetti

        // Alusta ikkuna jos se on tyhjä
        if world.window.start == world.window.end {
            let window_size = 4096.min(world.data.len());
            world.window = 0..window_size;
        }

        while current_quota > 0 {
            // Kysy schedulerilta mitä tehdä
            let pressure = world.data.len() as f64 / world.memory_limit as f64;
            let action = self.scheduler.decide_next_action(
                &self.stats,
                pressure,
                current_quota,
                self.processing_quota,
            );

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
                    // Kokeile ensin Dictionary-entry rakentamista (50% todennäköisyydellä)
                    let try_dictionary = rand::random::<f64>() < 0.5;
                    
                    if try_dictionary {
                        let slice = world.get_window_data();
                        if let Some(pattern) = self.build_dictionary_entry(slice, 4, 20) {
                            quota_cost = 10;
                            let pattern_id = pattern.id;
                            self.known_patterns.push(pattern);
                            self.stats.record_explore(quota_cost, 0); // Ei apply'd vielä, nolla gain tässä vaiheessa
                            println!("  ✓ Explore: Dictionary-sana #{} lisätty (word_id: {:?})", pattern_id, 
                                     match &self.known_patterns.last().unwrap().operator {
                                         Operator::Dictionary { word_id } => word_id,
                                         _ => &0,
                                     });
                            println!("  📚 PatternBank: {}/{} mallia muistissa", self.known_patterns.len(), self.pattern_bank_capacity);
                            self.forget_if_needed();
                        } else if let Some(patch) = self.explore(world) {
                            // Jos Dictionary ei löydy, yritä muita malleja
                            quota_cost = self.handle_explore_patch(world, evaluator, patch);
                        } else {
                            quota_cost = 10;
                            self.stats.record_explore(quota_cost, 0);
                        }
                    } else if let Some(patch) = self.explore(world) {
                        quota_cost = self.handle_explore_patch(world, evaluator, patch);
                    } else {
                        quota_cost = 10;
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

    /// Häivytä recent_gain-arvoja, jotta vanhentuneet mallit menettävät painoaan
    fn decay_recent_gains(&mut self) {
        const DECAY: f64 = 0.95;
        for pattern in &mut self.known_patterns {
            pattern.recent_gain *= DECAY;
            if pattern.recent_gain.abs() < 1e-6 {
                pattern.recent_gain = 0.0;
            }
        }
    }

    /// Etsi malleja malleista (hyvin yksinkertainen runko)
    fn meta_learn(&mut self) {
        println!("  🧠 MetaLearn: Tutkitaan PatternBankia...");

        let mut run_length_groups: HashMap<usize, Vec<(u32, f64, u8)>> = HashMap::new();
        for pattern in &self.known_patterns {
            if let Operator::RunLength(byte, min_len) = pattern.operator {
                run_length_groups
                    .entry(min_len)
                    .or_default()
                    .push((pattern.id, pattern.recent_gain, byte));
            }
        }

        let mut best_group: Option<(usize, Vec<(u32, f64, u8)>, usize, usize)> = None;

        for (min_len, entries) in run_length_groups.into_iter() {
            let mut unique_bytes = HashSet::new();
            for (_, _, b) in &entries {
                unique_bytes.insert(*b);
            }
            let unique_count = unique_bytes.len();
            if unique_count < 4 {
                continue;
            }
            let total_count = entries.len();
            let should_update = match &best_group {
                None => true,
                Some((best_min_len, _, best_unique, best_total)) => {
                    unique_count > *best_unique
                        || (unique_count == *best_unique && total_count > *best_total)
                        || (unique_count == *best_unique
                            && total_count == *best_total
                            && min_len < *best_min_len)
                }
            };
            if should_update {
                best_group = Some((min_len, entries, unique_count, total_count));
            }
        }

        if let Some((min_len, entries, unique_count, total_count)) = best_group {
            println!(
                "  🧠 MetaLearn: RunLength-ryhmä löydetty (min_len={}, uniikkia tavua {}, malleja {}).",
                min_len, unique_count, total_count
            );

            let has_general = self.known_patterns.iter().any(|p| {
                matches!(
                    p.operator,
                    Operator::GeneralizedRunLength { min_len: existing } if existing == min_len
                )
            });

            if !has_general {
                let meta_id = self.next_pattern_id;
                let mut meta_pattern = Pattern::new(meta_id, Operator::GeneralizedRunLength { min_len });
                if total_count > 0 {
                    let avg_recent = entries.iter().map(|(_, gain, _)| *gain).sum::<f64>() / total_count as f64;
                    meta_pattern.recent_gain = avg_recent;
                }
                meta_pattern.last_used = std::time::SystemTime::now();
                self.next_pattern_id += 1;
                self.known_patterns.push(meta_pattern);
                println!(
                    "  🧠 MetaLearn: Luotiin yleistetty RunLength-malli #{}, min_len {}.",
                    meta_id, min_len
                );
            } else {
                println!("  🧠 MetaLearn: Yleistetty RunLength(min_len={}) on jo olemassa.", min_len);
            }

            if total_count > 3 {
                let mut sorted = entries.clone();
                sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
                let remove_count = sorted.len() - 3;
                let remove_set: HashSet<u32> = sorted
                    .into_iter()
                    .take(remove_count)
                    .map(|(id, _, _)| id)
                    .collect();
                if !remove_set.is_empty() {
                    let before = self.known_patterns.len();
                    self.known_patterns.retain(|p| !remove_set.contains(&p.id));
                    let removed = before - self.known_patterns.len();
                    if removed > 0 {
                        println!(
                            "  🧠 MetaLearn: Poistettiin {} heikkoa RunLength-mallia (min_len={}).",
                            removed, min_len
                        );
                    }
                }
            }
        } else {
            println!("  🧠 MetaLearn: Ei löytynyt yleistettäviä RunLength-ryhmiä.");
        }

        // Varmista, että muistipankki pysyy kapasiteetin sisällä
        self.forget_if_needed();
    }

    /// Poista huonoin malli, jos PatternBank on täynnä
    fn forget_if_needed(&mut self) {
        if self.known_patterns.len() > self.pattern_bank_capacity {
            use std::time::SystemTime;
            let now = SystemTime::now();

            // Palkitse mallia, jota on käytetty *äskettäin* (viimeisen 60s aikana)
            // Tämä on paljon tärkeämpää kuin ikäpenalty.
            let recency_bonus = 60.0; 
            
            // Vanha historiallinen paino oli liian suuri ja tukki muistin.
            // Asetetaan se nollaan tai hyvin pieneksi.
            let legacy_weight = 0.0; // TAI 0.00001

            let mut worst_index = 0usize;
            let mut worst_score = f64::INFINITY;

            for (i, p) in self.known_patterns.iter().enumerate() {
                let age_secs = now.duration_since(p.last_used).unwrap_or_default().as_secs_f64();
                
                // UUSI LASKENTA:
                // Perustuu ensisijaisesti VIIMEAIKAISEEN hyötyyn.
                let mut score = p.recent_gain;

                // Anna pieni bonus, jos mallia on juuri käytetty, jotta se ei heti poistu
                if age_secs < recency_bonus {
                    score += p.recent_gain * 0.5; // Lisäbonus aktiivisesta käytöstä
                } else {
                    // Rankaise vain vanhoja, käyttämättömiä malleja
                    score -= (age_secs - recency_bonus) * 0.01; // Kevyt ikäpenalty
                }

                // Lisää (nyt merkityksetön) historiallinen paino
                score += (p.total_bytes_saved as f64) * legacy_weight;
                
                if score < worst_score {
                    worst_score = score;
                    worst_index = i;
                }
            }

            let removed = self.known_patterns.remove(worst_index);
            println!(
                "  🗑️ Forget: PatternBank täynnä. Poistettiin malli #{} (score: {:.2}, recent_gain {:.2}, ikä {:?}, käyttöjä {}).",
                removed.id,
                worst_score,
                removed.recent_gain,
                now.duration_since(removed.last_used).unwrap_or_default(),
                removed.usage_count
            );
        }
    }

    /// Päivitä kustannuskomponentit statsiin
    fn update_cost_breakdown(&mut self, world: &World, evaluator: &crate::evaluator::Evaluator) {
        let (c_models, c_residual) = evaluator.calculate_cost_breakdown(world);
        self.stats.update_costs(c_models, c_residual);
    }

    /// Tunnista operaattori Patchista (yksinkertaistettu)
    fn identify_operator(&self, patch: &Patch) -> Operator {
        use crate::operator::OP_DICT;
        if patch.new_data.len() == 3 && patch.new_data[0] == OP_RLE {
            Operator::RunLength(patch.new_data[1], patch.new_data[2] as usize)
        } else if patch.new_data.len() == 4 && patch.new_data[0] == OP_LZ {
            let dist = (patch.new_data[1] as usize) | ((patch.new_data[2] as usize) << 8);
            let len = patch.new_data[3] as usize;
            Operator::BackRef(dist, len)
        } else if patch.new_data.len() == 4 && patch.new_data[0] == OP_DELTA {
            let len = patch.new_data[1] as usize;
            let start = patch.new_data[2];
            let delta = patch.new_data[3] as i8;
            Operator::DeltaSequence { start, delta, len }
        } else if patch.new_data.len() >= 5 && patch.new_data[0] == OP_XOR {
            let len = (patch.new_data[1] as usize) | ((patch.new_data[2] as usize) << 8);
            let key_len = patch.new_data[3] as usize;
            if patch.new_data.len() >= 5 + key_len {
                let base = patch.new_data[4];
                let key = patch.new_data[5..(5 + key_len)].to_vec();
                Operator::XorMask { key, base, len }
            } else {
                Operator::RunLength(0, 0)
            }
        } else if patch.new_data.len() == 3 && patch.new_data[0] == OP_DICT {
            let word_id = (patch.new_data[1] as u32) | ((patch.new_data[2] as u32) << 8);
            Operator::Dictionary { word_id }
        } else {
            // Oletus: RunLength jos tuntematon
            Operator::RunLength(0, 0)
        }
    }

    /// Käsittele explore-patchin soveltaminen ja oppiminen
    fn handle_explore_patch(&mut self, world: &mut World, evaluator: &crate::evaluator::Evaluator, 
                            patch: Patch) -> u32 {
        // Muunna paikallinen patch globaaliksi
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
            let quota_cost = 10;
            self.stats.record_explore(quota_cost, 0);
            println!("  ✗ Explore: Malli hylätty (liian pieni hyöty: {} tavua)", gain);
            quota_cost
        } else {
            let quota_cost = 10;
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
            quota_cost
        }
    }

    /// Exploit: Käytä tunnettuja malleja
    /// Palauttaa: (Patch, pattern_id) jos löytyi sovellettava malli
    /// HUOM: Patch.range on paikallinen ikkunan suhteen!
    /// UUSI: Etsii parhaan mallin vain N parhaan (recent_gain) mallin joukosta.
    fn exploit(&self, data_slice: &[u8]) -> Option<(Patch, u32)> {
        let mut best_match: Option<(Patch, u32, usize)> = None; // (patch, pattern_id, saved_bytes)

        // --- UUSI OSA ALKAA ---
        const EXPLOIT_BEAM_WIDTH: usize = 100; // Testaa vain 100 parasta mallia

        // 1. Kerää ehdokkaat ja lajittele ne `recent_gain` mukaan
        let mut candidates: Vec<&Pattern> = self.known_patterns.iter().collect();
        // Lajitellaan laskevaan järjestykseen (paras ensin)
        candidates.sort_by(|a, b| b.recent_gain.partial_cmp(&a.recent_gain).unwrap_or(Ordering::Equal));

        // 2. Ota vain N parasta
        let patterns_to_check = candidates.into_iter().take(EXPLOIT_BEAM_WIDTH);
        // --- UUSI OSA LOPPUU ---

        // Käy läpi VAIN PARHAAT tunnetut mallit ja löydä paras
        for pattern in patterns_to_check {
            let candidate = match &pattern.operator {
                Operator::RunLength(byte, min_count) => {
                    self.find_run_length(data_slice, *byte, *min_count)
                        .map(|p| (p, pattern.id))
                }
                Operator::GeneralizedRunLength { min_len } => {
                    self.find_any_run_length(data_slice, *min_len)
                        .map(|p| (p, pattern.id))
                }
                Operator::BackRef(distance, length) => {
                    let dist = *distance;
                    let len = (*length).min(255);
                    let mut result = None;
                    if data_slice.len() >= len && dist > 0 {
                        let max_start = data_slice.len().saturating_sub(len);
                        for i in 0..=max_start {
                            if i < dist { continue; }
                            let src = i - dist;
                            if src + len > data_slice.len() { continue; }
                            if &data_slice[i..i+len] == &data_slice[src..src+len] {
                                let new_data = vec![
                                    OP_LZ,
                                    (dist & 0xFF) as u8,
                                    ((dist >> 8) & 0xFF) as u8,
                                    len as u8,
                                ];
                                let patch = Patch { range: i..(i + len), new_data };
                                result = Some((patch, pattern.id));
                                break;
                            }
                        }
                    }
                    result
                }
                Operator::DeltaSequence { start, delta, len } => {
                    self.find_delta_sequence_exact(data_slice, *start, *delta, *len)
                        .map(|p| (p, pattern.id))
                }
                Operator::XorMask { key, base, len } => {
                    self.find_xor_mask_exact(data_slice, key, *base, *len)
                        .map(|p| (p, pattern.id))
                }
                Operator::Dictionary { word_id } => {
                    self.find_dictionary_word(data_slice, *word_id)
                        .map(|p| (p, pattern.id))
                }
            };

            if let Some((patch, pid)) = candidate {
                let original_len = patch.range.len();
                let encoded_len = patch.new_data.len();
                let saved = original_len.saturating_sub(encoded_len);
                
                match &best_match {
                    None => best_match = Some((patch, pid, saved)),
                    Some((_, _, best_saved)) if saved > *best_saved => {
                        best_match = Some((patch, pid, saved));
                    }
                    _ => {}
                }
            }
        }

        best_match.map(|(patch, pid, _)| (patch, pid))
    }

    /// Explore: etsi PARAS malli ikkunasta, älä ensimmäistä
    fn explore(&mut self, world: &World) -> Option<Patch> {
        let slice = world.get_window_data();

        let mut best_patch: Option<Patch> = None;
        let mut max_gain: i32 = i32::MIN; // seurataan parasta nettohyötyä

        // Apuri: arvioi ehdokkaan hyöty (alkuperäinen koko - uusi koko) ja päivitä paras
        let mut consider = |patch: Patch| {
            let gain = patch.range.len() as i32 - patch.new_data.len() as i32;
            if gain > max_gain {
                max_gain = gain;
                best_patch = Some(patch);
            }
        };

        // 1. Pidemmät n-grammit (sanat, lauseet)
        if let Some(p) = self.find_ngram_reference(slice, 4, 20) { // 4-20 tavua (sanat)
            consider(p);
        }

        // 2. Backref (LZ-viittaukset)
        if let Some(p) = self.find_backref(world, 4, 16384) { // min len 4, max distance 16KB
            consider(p);
        }

        // 3. Delta-sekvenssit
        if let Some(p) = self.find_delta_sequence(slice, 6) {
            consider(p);
        }

        // 4. XOR-naamiot
        if let Some(p) = self.find_xor_mask(slice, 2, 8, 3) {
            consider(p);
        }

        // 5. Run-length (viimeisenä)
        if let Some(p) = self.find_any_run_length(slice, 3) {
            consider(p);
        }

        // Palauta vain se yksi paras löydös (MIN_ACCEPT_GAIN tarkistetaan myöhemmin)
        best_patch
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

    /// Etsi ikkunan sisällä toistuvia n-grammeja ja koodaa ne LZ-viittauksena
    /// PARANNETTU: Priorisoi pidempiä osumia (sanoja) lyhyiden yli
    fn find_ngram_reference(&self, data: &[u8], min_len: usize, max_len: usize) -> Option<Patch> {
        if data.len() < min_len * 2 {
            return None;
        }

        use std::collections::hash_map::Entry;

        let mut first_occurrence: HashMap<Vec<u8>, usize> = HashMap::new();
        let mut best: Option<(usize, usize, usize)> = None; // (target, source, len)

        // Etsi pidemmistä osumista lyhyempiin (priorisointi)
        for len in (min_len..=max_len).rev() {
            if len > 255 {
                continue;
            }
            
            for i in 0..data.len().saturating_sub(len) {
                let key = data[i..i + len].to_vec();
                match first_occurrence.entry(key) {
                    Entry::Vacant(v) => {
                        v.insert(i);
                    }
                    Entry::Occupied(entry) => {
                        let first = *entry.get();
                        if i <= first {
                            continue;
                        }
                        let distance = i - first;
                        if distance == 0 || distance > u16::MAX as usize {
                            continue;
                        }
                        let estimated_gain = len as i32 - 4; // OP_LZ koodauskustannus
                        if estimated_gain <= 0 {
                            continue;
                        }
                        match best {
                            None => best = Some((i, first, len)),
                            Some((_, _, best_len)) => {
                                // Priorisoi pidempiä osumia
                                if len > best_len {
                                    best = Some((i, first, len));
                                }
                            }
                        }
                    }
                }
            }
            
            // Jos löytyi hyvä pitkä osuma, käytä sitä
            if let Some((_, _, found_len)) = best {
                if found_len >= len {
                    break;
                }
            }
        }

        if let Some((target, source, len)) = best {
            let distance = target - source;
            let new_data = vec![
                OP_LZ,
                (distance & 0xFF) as u8,
                ((distance >> 8) & 0xFF) as u8,
                len as u8,
            ];
            return Some(Patch {
                range: target..(target + len),
                new_data,
            });
        }

        None
    }

    /// Etsi uutta delta-jonoa avoimesti eksplorointia varten
    fn find_delta_sequence(&self, data: &[u8], min_len: usize) -> Option<Patch> {
        if data.len() < min_len {
            return None;
        }

        let mut best: Option<(usize, u8, i8, usize)> = None; // (start, start_byte, delta, len)

        for start in 0..=data.len().saturating_sub(min_len) {
            if start + 1 >= data.len() {
                break;
            }

            let delta = data[start + 1].wrapping_sub(data[start]) as i8;
            let mut len = 2usize;
            let mut prev = data[start + 1];
            let mut idx = start + 2;

            while idx < data.len() && len < 255 {
                let expected = Solver::add_delta(prev, delta);
                if data[idx] == expected {
                    len += 1;
                    prev = expected;
                    idx += 1;
                } else {
                    break;
                }
            }

            if len >= min_len {
                let gain = len as i32 - 4;
                if gain > 0 {
                    match best {
                        None => best = Some((start, data[start], delta, len)),
                        Some((_, _, _, best_len)) if len > best_len => {
                            best = Some((start, data[start], delta, len));
                        }
                        _ => {}
                    }
                }
            }
        }

        if let Some((start, init, delta, len)) = best {
            let new_data = vec![OP_DELTA, len as u8, init, delta as u8];
            return Some(Patch {
                range: start..(start + len),
                new_data,
            });
        }

        None
    }

    /// Tarkastele onko data-alueella valmiiksi tunnettu delta-jono
    fn find_delta_sequence_exact(&self, data: &[u8], start_byte: u8, delta: i8, len: usize) -> Option<Patch> {
        if len < 2 || len > 255 || data.len() < len {
            return None;
        }

        for start in 0..=data.len().saturating_sub(len) {
            if data[start] != start_byte {
                continue;
            }
            let mut prev = data[start];
            let mut ok = true;
            for offset in 1..len {
                let expected = Solver::add_delta(prev, delta);
                if data[start + offset] != expected {
                    ok = false;
                    break;
                }
                prev = expected;
            }
            if ok {
                let new_data = vec![OP_DELTA, len as u8, start_byte, delta as u8];
                return Some(Patch {
                    range: start..(start + len),
                    new_data,
                });
            }
        }

        None
    }

    /// Etsi XOR-naamioituja jaksoja, joissa pohja on vakio
    fn find_xor_mask(&self, data: &[u8], key_min: usize, key_max: usize, min_repeats: usize) -> Option<Patch> {
        if data.len() < key_min * min_repeats {
            return None;
        }

        let mut best: Option<(usize, usize, Vec<u8>, u8)> = None; // (start, len, key, base)

        for start in 0..data.len() {
            for key_len in key_min..=key_max {
                if key_len == 0 || start + key_len > data.len() {
                    break;
                }

                let key = &data[start..start + key_len];
                let base = data[start] ^ key[0];
                let mut len = key_len;
                while start + len < data.len() && len < u16::MAX as usize {
                    let key_byte = key[len % key_len];
                    if data[start + len] ^ key_byte == base {
                        len += 1;
                    } else {
                        break;
                    }
                }

                if len >= key_len * min_repeats {
                    let gain = len as i32 - (5 + key_len) as i32;
                    if gain > 0 {
                        match &best {
                            None => best = Some((start, len, key.to_vec(), base)),
                            Some((_, best_len, _, _)) if len > *best_len => {
                                best = Some((start, len, key.to_vec(), base));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        if let Some((start, len, key, base)) = best {
            let mut new_data = Vec::with_capacity(5 + key.len());
            new_data.push(OP_XOR);
            new_data.push((len & 0xFF) as u8);
            new_data.push(((len >> 8) & 0xFF) as u8);
            new_data.push(key.len() as u8);
            new_data.push(base);
            new_data.extend_from_slice(&key);
            return Some(Patch {
                range: start..(start + len),
                new_data,
            });
        }

        None
    }

    /// Hae tunnettu XOR-naamioitu jakso
    fn find_xor_mask_exact(&self, data: &[u8], key: &[u8], base: u8, len: usize) -> Option<Patch> {
        if key.is_empty() || data.len() < len || len > u16::MAX as usize {
            return None;
        }

        for start in 0..=data.len().saturating_sub(len) {
            let mut ok = true;
            for offset in 0..len {
                let key_byte = key[offset % key.len()];
                if data[start + offset] ^ key_byte != base {
                    ok = false;
                    break;
                }
            }
            if ok {
                let mut new_data = Vec::with_capacity(5 + key.len());
                new_data.push(OP_XOR);
                new_data.push((len & 0xFF) as u8);
                new_data.push(((len >> 8) & 0xFF) as u8);
                new_data.push(key.len() as u8);
                new_data.push(base);
                new_data.extend_from_slice(key);
                return Some(Patch {
                    range: start..(start + len),
                    new_data,
                });
            }
        }

        None
    }

    #[inline]
    fn add_delta(value: u8, delta: i8) -> u8 {
        (((value as i16) + (delta as i16)).rem_euclid(256)) as u8
    }

    /// Rakenna sanakirja ladatuista Dictionary-malleista
    /// HUOM: Tämä on väliaikainen, sillä ladattaessa ei ole varsinaisia sanoja,
    /// vain word_id:t. Todellisuudessa Dictionary-mallit tulisi rakentaa uudelleen
    /// datasta, mutta nyt tyhjennämme sanakirjan ladattaessa.
    fn rebuild_dictionary(&mut self) {
        self.dictionary.clear();
        self.word_to_id.clear();
        self.next_word_id = 0;
        // TODO: Jos haluamme säilyttää Dictionary-malleja, pitää tallentaa myös sanakirja
        // tai rakentaa se uudelleen datasta
    }

    /// Etsi sanakirjasta word_id:tä vastaava sana ja skannaa data sitä vastaan
    fn find_dictionary_word(&self, data: &[u8], word_id: u32) -> Option<Patch> {
        let word = self.dictionary.get(&word_id)?;
        let word_len = word.len();
        
        if word_len == 0 || word_len > data.len() {
            return None;
        }

        // Etsi ensimmäinen esiintymä
        for start in 0..=data.len().saturating_sub(word_len) {
            if &data[start..start + word_len] == word.as_slice() {
                use crate::operator::OP_DICT;
                let new_data = vec![
                    OP_DICT,
                    (word_id & 0xFF) as u8,
                    ((word_id >> 8) & 0xFF) as u8,
                ];
                return Some(Patch {
                    range: start..(start + word_len),
                    new_data,
                });
            }
        }

        None
    }

    /// Rakenna uusi Dictionary-entry: etsi toistuva 4-20 tavun sekvenssi (sana),
    /// lisää sanakirjaan ja luo Pattern
    fn build_dictionary_entry(&mut self, data: &[u8], min_len: usize, max_len: usize) -> Option<Pattern> {
        let max_len = max_len.min(data.len());
        
        // Etsi toistuvia sekvenssejä (vähintään 2 esiintymää)
        let mut best: Option<(Vec<u8>, usize)> = None; // (word, count)
        
        for len in (min_len..=max_len).rev() { // Aloita pisimmistä
            if len > data.len() {
                continue;
            }
            
            let mut seen = HashMap::new();
            for start in 0..=data.len().saturating_sub(len) {
                let seq = &data[start..start + len];
                *seen.entry(seq.to_vec()).or_insert(0) += 1;
            }
            
            for (seq, count) in seen {
                if count >= 2 {
                    match &best {
                        None => best = Some((seq, count)),
                        Some((_, best_count)) if count > *best_count => {
                            best = Some((seq, count));
                        }
                        _ => {}
                    }
                }
            }
            
            if best.is_some() {
                break; // Löytyi pidempi, ei tarvitse etsiä lyhyempiä
            }
        }
        
        let (word, _occurrences) = best?;
        
        // Tarkista onko sana jo sanakirjassa
        if let Some(&existing_id) = self.word_to_id.get(&word) {
            // Sana on jo olemassa, käytä sitä
            let pattern_id = self.next_pattern_id;
            self.next_pattern_id += 1;
            
            let pattern = Pattern::new(
                pattern_id,
                Operator::Dictionary { word_id: existing_id },
            );
            return Some(pattern);
        }
        
        // Lisää uusi sana sanakirjaan
        let word_id = self.next_word_id;
        self.next_word_id += 1;
        self.dictionary.insert(word_id, word.clone());
        self.word_to_id.insert(word.clone(), word_id);
        
        let pattern_id = self.next_pattern_id;
        self.next_pattern_id += 1;
        
        Some(Pattern::new(
            pattern_id,
            Operator::Dictionary { word_id },
        ))
    }
}
