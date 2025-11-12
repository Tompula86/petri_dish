use crate::operator::{OP_DELTA, OP_DICT, OP_GRAMMAR, OP_LZ, OP_RLE, OP_XOR, Operator};
use crate::pattern::Pattern;
use crate::stats::Stats;
use crate::world::{Patch, World};
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
    /// Sanakirja: word_id -> word metadata
    dictionary_words: HashMap<u32, DictionaryWord>,
    /// Käänteinen sanakirja: word bytes -> word_id; nopeaan hakuun
    dictionary_lookup: HashMap<Vec<u8>, u32>,
    /// Seuraava vapaa word_id
    next_word_id: u32,
    /// Grammar-säännöt meta-oppimista varten
    #[allow(dead_code)]
    grammar_rules: HashMap<u32, GrammarRule>,
    #[allow(dead_code)]
    grammar_lookup: HashMap<Vec<OperatorKey>, u32>,
    #[allow(dead_code)]
    next_grammar_id: u32,
    /// Ikkunan hallinta
    window_size: usize,
    window_stride: usize,
    current_window_start: usize,
    zero_gain_streak: u32,
    window_cap_fraction: f64,
    last_world_len: usize,
    /// Cycle-laskuri jotta voidaan tehdä harvemmin päivittyviä operaatioita
    cycle_count: u32,
}

impl Solver {
    const MIN_ACCEPT_GAIN: i32 = 3; // Salli vieläkin pienemmät hyödyt (aiemmin 5, alussa 25)
    const PATTERN_BANK_FILE: &'static str = "pattern_bank.json";
    const DICTIONARY_MIN_LEN: usize = 3; // Lyhyemmät sanat (aiemmin 4)
    const DICTIONARY_MAX_LEN: usize = 64; // Pidemmät lauseet (aiemmin 32)
    const DICTIONARY_CAPACITY: usize = 512; // Enemmän sanoja/lauseita (aiemmin 256)
    const DICTIONARY_MATCH_LIMIT: usize = 128; // Enemmän matcheja (aiemmin 64)
    #[allow(dead_code)]
    const GRAMMAR_MIN_SEQ: usize = 2;
    #[allow(dead_code)]
    const GRAMMAR_MAX_SEQ: usize = 4;

    pub fn new(processing_quota: u32, pattern_bank_capacity: usize, window_fraction: f64) -> Self {
        const BASE_WINDOW_SIZE: usize = 128;
        let initial_stride = (BASE_WINDOW_SIZE / 2).max(32);

        Solver {
            processing_quota,
            known_patterns: Vec::new(),
            next_pattern_id: 0,
            pattern_bank_capacity,
            stats: Stats::new(),
            scheduler: crate::scheduler::Scheduler::new(),
            dictionary_words: HashMap::new(),
            dictionary_lookup: HashMap::new(),
            next_word_id: 0,
            grammar_rules: HashMap::new(),
            grammar_lookup: HashMap::new(),
            next_grammar_id: 0,
            window_size: BASE_WINDOW_SIZE,
            window_stride: initial_stride,
            current_window_start: 0,
            zero_gain_streak: 0,
            window_cap_fraction: window_fraction.clamp(0.1, 1.0),
            last_world_len: 0,
            cycle_count: 0,
        }
    }

    /// Lataa Solver aiemmin tallennetuista malleista, tai luo uuden jos tiedostoa ei ole
    pub fn load_or_new(
        processing_quota: u32,
        pattern_bank_capacity: usize,
        window_fraction: f64,
    ) -> Self {
        if let Ok(contents) = std::fs::read_to_string(Self::PATTERN_BANK_FILE) {
            if let Ok(patterns) = serde_json::from_str::<Vec<Pattern>>(&contents) {
                let max_id = patterns.iter().map(|p| p.id).max().unwrap_or(0);
                println!(
                    "  ♻️  Ladattiin {} mallia tiedostosta {}",
                    patterns.len(),
                    Self::PATTERN_BANK_FILE
                );
                let mut solver = Solver {
                    processing_quota,
                    known_patterns: patterns,
                    next_pattern_id: max_id + 1,
                    pattern_bank_capacity,
                    stats: Stats::new(),
                    scheduler: crate::scheduler::Scheduler::new(),
                    dictionary_words: HashMap::new(),
                    dictionary_lookup: HashMap::new(),
                    next_word_id: 0,
                    grammar_rules: HashMap::new(),
                    grammar_lookup: HashMap::new(),
                    next_grammar_id: 0,
                    window_size: 128,
                    window_stride: 64,
                    current_window_start: 0,
                    zero_gain_streak: 0,
                    window_cap_fraction: window_fraction.clamp(0.1, 1.0),
                    last_world_len: 0,
                    cycle_count: 0,
                };
                // Rakennetaan sanakirja ladatuista Dictionary-malleista
                solver.rebuild_dictionary();
                return solver;
            }
        }
        println!("  🆕 Luodaan uusi Solver (ei aiempia malleja)");
        Self::new(processing_quota, pattern_bank_capacity, window_fraction)
    }

    /// Tallenna tunnetut mallit levylle
    pub fn save_patterns(&self) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(&self.known_patterns)?;
        std::fs::write(Self::PATTERN_BANK_FILE, json)?;
        println!(
            "  💾 Tallennettu {} mallia tiedostoon {}",
            self.known_patterns.len(),
            Self::PATTERN_BANK_FILE
        );
        Ok(())
    }

    /// Pääsilmukka: Solver operoi Worldissä
    /// VAIHE 6: Quota-pohjainen silmukka, scheduler-ohjattu
    pub fn live(&mut self, world: &mut World, evaluator: &crate::evaluator::Evaluator) {
        // Nollaa syklin tilastot
        self.stats.reset_cycle();
        self.decay_recent_gains();
        let mut current_quota = self.processing_quota; // Ota syklin budjetti

        // Päivitä cycle-laskuri
        self.cycle_count += 1;

        self.refresh_focus_window(world);

        // Päivitä sanakirja harvemmin - anna patternien ehtiä muodostua!
        if self.cycle_count <= 40 {
            // Alussa useammin (joka 10. sykli)
            if self.cycle_count % 10 == 0 {
                self.refresh_dictionary(world);
            }
        } else {
            // Myöhemmin harvemmin (joka 20. sykli)
            if self.cycle_count % 20 == 0 {
                self.refresh_dictionary(world);
            }
        }

        while current_quota > 0 {
            self.refresh_focus_window(world);
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
                            if let Some(pattern) =
                                self.known_patterns.iter_mut().find(|p| p.id == pattern_id)
                            {
                                pattern.record_usage(gain, self.cycle_count);
                                println!(
                                    "  ✓ Exploit onnistui (pattern #{}, säästö: {} tavua, käytetty {} kertaa)",
                                    pattern_id, gain, pattern.usage_count
                                );
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
                }

                crate::scheduler::Action::Explore => {
                    // --- KORJAUS: Poistetaan buginen Dictionary-logiikka käytöstä ---
                    // Dictionary täyttää PatternBankin 0-hyödyn malleilla ennen kuin ne on testattu.
                    // Kaikki mallit täytyy pakottaa todistamaan arvonsa ennen muistiin pääsyä.
                    let try_dictionary = false; // Alkuperäinen: rand::random::<f64>() < 0.5;

                    if try_dictionary {
                        let slice = world.get_window_data();
                        if let Some(pattern) = self.build_dictionary_entry(slice, 4, 20) {
                            quota_cost = 10;
                            let pattern_id = pattern.id;
                            self.known_patterns.push(pattern);
                            self.stats.record_explore(quota_cost, 0); // Ei apply'd vielä, nolla gain tässä vaiheessa
                            println!(
                                "  ✓ Explore: Dictionary-sana #{} lisätty (word_id: {:?})",
                                pattern_id,
                                match &self.known_patterns.last().unwrap().operator {
                                    Operator::Dictionary { word_id } => word_id,
                                    _ => &0,
                                }
                            );
                            println!(
                                "  📚 PatternBank: {}/{} mallia muistissa",
                                self.known_patterns.len(),
                                self.pattern_bank_capacity
                            );
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
                }

                crate::scheduler::Action::ShiftWindow => {
                    self.advance_window(world);
                    self.refresh_focus_window(world);
                    quota_cost = 2;
                    self.stats.record_seek(quota_cost);
                    let window = &world.window;
                    println!(
                        "  🔍 ShiftWindow: uusi ikkuna {}..{} ({} tavua)",
                        window.start,
                        window.end,
                        window.len()
                    );
                }

                crate::scheduler::Action::MetaLearn => {
                    // Meta-oppiminen on kallista
                    quota_cost = 100;
                    if current_quota >= quota_cost {
                        println!("  🧠 MetaLearn: Käynnistetään meta-oppiminen...");
                        self.meta_learn();
                        self.stats.record_meta(quota_cost, 0);
                    } else {
                        println!(
                            "  🧠 MetaLearn: Ei tarpeeksi quotaa (tarvitaan {})",
                            quota_cost
                        );
                    }
                }
            }

            if current_quota >= quota_cost {
                current_quota -= quota_cost;
            } else {
                break; // Quota loppui tältä sykliltä
            }
        }

        if self.stats.total_gain() <= 0 {
            self.zero_gain_streak = self.zero_gain_streak.saturating_add(1);
        } else {
            self.zero_gain_streak = 0;
        }

        self.adjust_window_size_after_cycle(world);
        self.refresh_focus_window(world);

        // REKURSIIVINEN PAKKAUS: Etsi malleja jo pakatusta datasta
        let world_pressure = world.data.len() as f64 / world.memory_limit as f64;

        // AGGRESSIVE CASCADE REPACK vain kun world lähes täynnä (>92%)
        if world_pressure > 0.92 && world.data.len() >= 500 {
            println!(
                "  ⚠️ World lähes täynnä ({:.1}%) - cascade repack!",
                world_pressure * 100.0
            );
            self.repack_compressed_data(world, evaluator);
        }
        // NORMAL REPACK HARVEMMIN - anna datan kasvaa ensin!
        else {
            let should_repack = if self.cycle_count <= 60 {
                // Alussa kerran per 20 sykliä
                self.cycle_count % 20 == 0
            } else if self.cycle_count <= 120 {
                // Keskivaiheessa kerran per 30 sykliä
                self.cycle_count % 30 == 0
            } else {
                // Loppuvaiheessa kerran per 50 sykliä
                self.cycle_count % 50 == 0
            };

            if should_repack && world.data.len() >= 500 {
                self.repack_compressed_data(world, evaluator);
            }
        }

        self.update_cost_breakdown(world, evaluator);
        let adjusted_world_pressure = world.data.len() as f64 / world.memory_limit as f64;
        self.prune_stale_patterns(adjusted_world_pressure);
    }

    /// Päivitä tarkennusikkuna systemaattiseen rullaustilaan
    fn refresh_focus_window(&mut self, world: &mut World) {
        if world.data.is_empty() {
            world.window = 0..0;
            self.current_window_start = 0;
            self.last_world_len = 0;
            return;
        }

        let data_len = world.data.len();
        if data_len > self.last_world_len {
            // Uutta dataa saapunut: hyppää lähelle loppua, jotta uudet sanomat käsitellään nopeasti
            let tail_start = data_len.saturating_sub(self.window_size);
            if tail_start > self.current_window_start {
                self.current_window_start = tail_start;
            }
            self.last_world_len = data_len;
        }

        self.window_size = self.window_size.min(data_len).max(64);
        self.window_stride = (self.window_size / 2).max(32);

        if self.window_size >= data_len {
            world.window = 0..data_len;
            self.current_window_start = 0;
            return;
        }

        if self.current_window_start + self.window_size > data_len {
            self.current_window_start = data_len.saturating_sub(self.window_size);
        }

        let start = self.current_window_start;
        let end = (start + self.window_size).min(data_len);
        let adjusted_start = if end - start < self.window_size {
            end.saturating_sub(self.window_size)
        } else {
            start
        };

        self.current_window_start = adjusted_start;
        world.window =
            adjusted_start..(adjusted_start + self.window_size.min(data_len - adjusted_start));
    }

    /// Siirrä ikkunaa eteenpäin systemaattisesti
    fn advance_window(&mut self, world: &World) {
        if world.data.is_empty() {
            self.current_window_start = 0;
            return;
        }

        let data_len = world.data.len();
        if self.window_size >= data_len {
            self.current_window_start = 0;
            return;
        }

        let stride = self.window_stride.max(1);
        let mut next_start = self.current_window_start.saturating_add(stride);
        if next_start + self.window_size >= data_len {
            next_start = 0;
        }
        self.current_window_start = next_start;
    }

    fn adjust_window_size_after_cycle(&mut self, world: &World) {
        const BASE_WINDOW_SIZE: usize = 128;
        const ZERO_STREAK_THRESHOLD: u32 = 3;
        const GROWTH_FACTOR: f64 = 1.5;

        let mut max_allowed =
            ((world.memory_limit as f64) * self.window_cap_fraction).round() as usize;
        max_allowed = max_allowed.max(BASE_WINDOW_SIZE);

        if self.zero_gain_streak >= ZERO_STREAK_THRESHOLD && self.window_size < max_allowed {
            let expanded = (self.window_size as f64 * GROWTH_FACTOR).round() as usize;
            self.window_size = expanded
                .min(max_allowed)
                .min(world.memory_limit)
                .max(BASE_WINDOW_SIZE);
            self.window_stride = (self.window_size / 2).max(32);
            self.zero_gain_streak = 0;
        }

        if world.data.len() < self.window_size {
            self.window_size = world.data.len().max(BASE_WINDOW_SIZE.min(world.data.len()));
            self.window_stride = (self.window_size / 2).max(32);
            self.current_window_start = self
                .current_window_start
                .min(world.data.len().saturating_sub(self.window_size));
        }
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
                run_length_groups.entry(min_len).or_default().push((
                    pattern.id,
                    pattern.recent_gain,
                    byte,
                ));
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
                let mut meta_pattern = Pattern::new(
                    meta_id,
                    Operator::GeneralizedRunLength { min_len },
                    self.cycle_count,
                );
                if total_count > 0 {
                    let avg_recent =
                        entries.iter().map(|(_, gain, _)| *gain).sum::<f64>() / total_count as f64;
                    meta_pattern.recent_gain = avg_recent;
                }
                meta_pattern.last_used = std::time::SystemTime::now();
                meta_pattern.last_used_cycle = self.cycle_count;
                self.next_pattern_id += 1;
                self.known_patterns.push(meta_pattern);
                println!(
                    "  🧠 MetaLearn: Luotiin yleistetty RunLength-malli #{}, min_len {}.",
                    meta_id, min_len
                );
            } else {
                println!(
                    "  🧠 MetaLearn: Yleistetty RunLength(min_len={}) on jo olemassa.",
                    min_len
                );
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

    /// Poista huonoimmat mallit, jos PatternBank on täynnä
    fn forget_if_needed(&mut self) {
        if self.known_patterns.len() <= self.pattern_bank_capacity {
            return;
        }

        while self.known_patterns.len() > self.pattern_bank_capacity {
            let current_cycle = self.cycle_count;
            let mut worst: Option<(usize, u32, f64)> = None; // (index, idle_cycles, score)

            for (i, pattern) in self.known_patterns.iter().enumerate() {
                let baseline_cycle = if pattern.last_used_cycle != 0 {
                    pattern.last_used_cycle
                } else if pattern.creation_cycle != 0 {
                    pattern.creation_cycle
                } else {
                    current_cycle
                };

                let idle_cycles = current_cycle.saturating_sub(baseline_cycle);

                // Painota viimeaikaista hyötyä, historiallista säästöä ja käyttötiheyttä.
                let usage_bonus = (pattern.usage_count as f64).sqrt() * 0.5;
                let historical_bonus = (pattern.total_bytes_saved.max(0) as f64).sqrt() * 0.15;
                let recent = pattern.recent_gain;
                let idle_penalty = idle_cycles as f64 * 0.35;
                let score = recent + usage_bonus + historical_bonus - idle_penalty;

                match &mut worst {
                    None => worst = Some((i, idle_cycles, score)),
                    Some((_, _, current_score)) if score < *current_score => {
                        worst = Some((i, idle_cycles, score));
                    }
                    _ => {}
                }
            }

            if let Some((index, idle_cycles, score)) = worst {
                let removed = self.known_patterns.remove(index);
                println!(
                    "  🗑️ Forget: PatternBank täynnä. Poistettiin malli #{} (score {:.2}, recent {:.2}, usage {}, idle {} sykliä).",
                    removed.id, score, removed.recent_gain, removed.usage_count, idle_cycles
                );
            } else {
                break;
            }
        }
    }

    /// Aggressiivinen siivous: poista pitkään käyttämättömät mallit, vaikka kapasiteettia olisi
    fn prune_stale_patterns(&mut self, world_pressure: f64) {
        if self.known_patterns.is_empty() {
            return;
        }

        // Anna järjestelmän lämmetä ennen kuin aloitetaan aggressiivinen siivous
        if self.cycle_count < 30 {
            return;
        }

        let current_cycle = self.cycle_count;
        // Kovempi paine maailman ollessa täynnä tai gainien sakatessa
        let high_pressure = world_pressure > 0.88 || self.zero_gain_streak >= 4;
        let idle_limit = if high_pressure { 30 } else { 60 };
        let cold_limit = if high_pressure { 12 } else { 25 };

        let mut removed = 0u32;
        self.known_patterns.retain(|pattern| {
            // Historiallisten mallien tiedot voivat olla puutteellisia -> anna armoa boot-vaiheessa
            if pattern.last_used_cycle == 0 && pattern.creation_cycle == 0 {
                return current_cycle < idle_limit;
            }

            let baseline_cycle = if pattern.last_used_cycle != 0 {
                pattern.last_used_cycle
            } else {
                pattern.creation_cycle
            };

            let idle_cycles = current_cycle.saturating_sub(baseline_cycle);
            let stale_recent = pattern.recent_gain < 0.5;
            let cold = pattern.usage_count <= 1;
            let negative_recent = pattern.recent_gain <= 0.0;

            let should_drop = (stale_recent && idle_cycles > idle_limit)
                || (cold && idle_cycles > cold_limit)
                || (negative_recent && idle_cycles > cold_limit);

            if should_drop {
                removed += 1;
                false
            } else {
                true
            }
        });

        if removed > 0 {
            println!(
                "  🧹 Forget: Puhdistettiin {} vanhaa mallia ({} jäljellä).",
                removed,
                self.known_patterns.len()
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
        } else if patch.new_data.len() == 3 && patch.new_data[0] == OP_GRAMMAR {
            let rule_id = (patch.new_data[1] as u32) | ((patch.new_data[2] as u32) << 8);
            Operator::GrammarRule { rule_id }
        } else {
            // Oletus: RunLength jos tuntematon
            Operator::RunLength(0, 0)
        }
    }

    /// Käsittele explore-patchin soveltaminen ja oppiminen
    fn handle_explore_patch(
        &mut self,
        world: &mut World,
        evaluator: &crate::evaluator::Evaluator,
        patch: Patch,
    ) -> u32 {
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
            println!(
                "  ✗ Explore: Malli hylätty (liian pieni hyöty: {} tavua)",
                gain
            );
            quota_cost
        } else {
            let quota_cost = 10;
            self.stats.record_explore(quota_cost, gain);

            // OPI: Lisää PatternBankiin
            let mut pattern = Pattern::new(self.next_pattern_id, operator, self.cycle_count);
            pattern.record_usage(gain, self.cycle_count);
            let pattern_id = self.next_pattern_id;
            self.next_pattern_id += 1;
            self.known_patterns.push(pattern);
            println!(
                "  ✓ Explore: Uusi malli #{} löydetty (säästö: {} tavua)",
                pattern_id, gain
            );
            println!(
                "  📚 PatternBank: {}/{} mallia muistissa",
                self.known_patterns.len(),
                self.pattern_bank_capacity
            );
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
        const EXPLOIT_BEAM_WIDTH: usize = 350; // Testaa 350 parasta mallia (aiemmin 200) - varmistaa että kaikki 300 mallia pankissa käydään läpi

        // 1. Kerää ehdokkaat ja PRIORISOI DICTIONARY-MALLIT
        let mut dict_patterns: Vec<&Pattern> = Vec::new();
        let mut other_patterns: Vec<&Pattern> = Vec::new();

        for pattern in &self.known_patterns {
            match pattern.operator {
                Operator::Dictionary { .. } => dict_patterns.push(pattern),
                _ => other_patterns.push(pattern),
            }
        }

        // Lajittele Dictionary-mallit estimated_gain mukaan (suurempi ensin)
        dict_patterns.sort_by(|a, b| {
            b.recent_gain
                .partial_cmp(&a.recent_gain)
                .unwrap_or(Ordering::Equal)
        });

        // Lajittele muut mallit normaalisti
        other_patterns.sort_by(|a, b| {
            b.recent_gain
                .partial_cmp(&a.recent_gain)
                .unwrap_or(Ordering::Equal)
        });

        // 2. KAIKKI Dictionary-mallit ensin, sitten muut beam widthiin asti
        let dict_count = dict_patterns.len();
        if dict_count > 0 {
            println!(
                "  🔎 Exploit: {} Dictionary-mallia, {} muuta mallia",
                dict_count,
                other_patterns.len()
            );
        }
        let patterns_to_check = dict_patterns.into_iter().chain(
            other_patterns
                .into_iter()
                .take(EXPLOIT_BEAM_WIDTH.saturating_sub(dict_count)),
        );
        // --- UUSI OSA LOPPUU ---

        // Käy läpi VAIN PARHAAT tunnetut mallit ja löydä paras
        for pattern in patterns_to_check {
            let candidate = match &pattern.operator {
                Operator::RunLength(byte, min_count) => self
                    .find_run_length(data_slice, *byte, *min_count)
                    .map(|p| (p, pattern.id)),
                Operator::GeneralizedRunLength { min_len } => self
                    .find_any_run_length(data_slice, *min_len)
                    .map(|p| (p, pattern.id)),
                Operator::BackRef(distance, length) => {
                    let dist = *distance;
                    let len = (*length).min(255);
                    let mut result = None;
                    if data_slice.len() >= len && dist > 0 {
                        let max_start = data_slice.len().saturating_sub(len);
                        for i in 0..=max_start {
                            if i < dist {
                                continue;
                            }
                            let src = i - dist;
                            if src + len > data_slice.len() {
                                continue;
                            }
                            if &data_slice[i..i + len] == &data_slice[src..src + len] {
                                let new_data = vec![
                                    OP_LZ,
                                    (dist & 0xFF) as u8,
                                    ((dist >> 8) & 0xFF) as u8,
                                    len as u8,
                                ];
                                let patch = Patch {
                                    range: i..(i + len),
                                    new_data,
                                };
                                result = Some((patch, pattern.id));
                                break;
                            }
                        }
                    }
                    result
                }
                Operator::BackRefRange { .. } => {
                    // Meta-operaattori: varsinainen backrefin valinta tapahtuu exploratiossa
                    None
                }
                Operator::DeltaSequence { start, delta, len } => self
                    .find_delta_sequence_exact(data_slice, *start, *delta, *len)
                    .map(|p| (p, pattern.id)),
                Operator::XorMask { key, base, len } => self
                    .find_xor_mask_exact(data_slice, key, *base, *len)
                    .map(|p| (p, pattern.id)),
                Operator::Dictionary { word_id } => self
                    .find_dictionary_word(data_slice, *word_id)
                    .map(|p| (p, pattern.id)),
                Operator::GrammarRule { .. } => {
                    // Grammar-operaattorit eivät vielä ole käytössä eksploitatiossa
                    None
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

        if let Some(p) = self.find_dictionary_patch(slice) {
            consider(p);
        }

        // 1. Pidemmät n-grammit (sanat, lauseet)
        if let Some(p) = self.find_ngram_reference(slice, 4, 20) {
            // 4-20 tavua (sanat)
            consider(p);
        }

        // 1b. Lyhyemmät n-grammit (3-byte patterns jotka toistuvat usein)
        if let Some(p) = self.find_ngram_reference(slice, 3, 6) {
            // 3-6 tavua (lyhyet toistot)
            consider(p);
        }

        // 2. Backref (LZ-viittaukset)
        if let Some(p) = self.find_backref(world, 3, 16384) {
            // min len 3 (aiemmin 4), max distance 16KB
            consider(p);
        }

        // 3. Delta-sekvenssit
        if let Some(p) = self.find_delta_sequence(slice, 5) {
            // 5 tavua (aiemmin 6) - salli lyhyemmät sekvenssit
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

        // 6. Lyhyet run-lengthit (2-byte runs voivat olla hyödyllisiä)
        if let Some(p) = self.find_any_run_length(slice, 2) {
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
        if we - ws < min_len + 1 {
            return None;
        }

        let mut best_target = 0usize;
        let mut best_src = 0usize;
        let mut best_len = 0usize;

        // Tutki muutama kohde ikkunassa (alku, 1/4, 1/2, 3/4)
        let span = we - ws;
        let candidates = [ws + 1, ws + span / 4, ws + span / 2, ws + (3 * span / 4)]
            .into_iter()
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
                    if l == max_len_possible {
                        break;
                    }
                }
            }
        }

        if best_len >= min_len {
            let distance = best_target - best_src;
            let enc_len = best_len.min(255);
            let new_data = vec![
                OP_LZ,
                (distance & 0xFF) as u8,
                ((distance >> 8) & 0xFF) as u8,
                enc_len as u8,
            ];
            // Paikallinen range offset = best_target - ws
            let local_start = best_target - ws;
            return Some(Patch {
                range: local_start..(local_start + enc_len),
                new_data,
            });
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
    fn find_delta_sequence_exact(
        &self,
        data: &[u8],
        start_byte: u8,
        delta: i8,
        len: usize,
    ) -> Option<Patch> {
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
    fn find_xor_mask(
        &self,
        data: &[u8],
        key_min: usize,
        key_max: usize,
        min_repeats: usize,
    ) -> Option<Patch> {
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
        self.dictionary_words.clear();
        self.dictionary_lookup.clear();
        self.next_word_id = 0;
        // TODO: Jos haluamme säilyttää Dictionary-malleja, pitää tallentaa myös sanakirja
        // tai rakentaa se uudelleen datasta
    }

    /// Etsi sanakirjasta word_id:tä vastaava sana ja skannaa data sitä vastaan
    fn find_dictionary_word(&self, data: &[u8], word_id: u32) -> Option<Patch> {
        let word = self.dictionary_words.get(&word_id)?;
        let word_len = word.bytes.len();

        if word_len == 0 || word_len > data.len() {
            return None;
        }

        // Etsi ensimmäinen esiintymä
        for start in 0..=data.len().saturating_sub(word_len) {
            if &data[start..start + word_len] == word.bytes.as_slice() {
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
    fn build_dictionary_entry(
        &mut self,
        _data: &[u8],
        _min_len: usize,
        _max_len: usize,
    ) -> Option<Pattern> {
        // Vanha logiikka korvataan Dictionary 2.0 -virralla; jätetään paluu None, jotta satunnaiset kutsut eivät riko logiikkaa.
        None
    }

    fn refresh_dictionary(&mut self, world: &World) {
        let data = &world.data;
        if data.len() < Self::DICTIONARY_MIN_LEN * 2 {
            return;
        }

        let mut counts: HashMap<Vec<u8>, WordStats> = HashMap::new();
        for len in Self::DICTIONARY_MIN_LEN..=Self::DICTIONARY_MAX_LEN {
            if len > data.len() {
                break;
            }
            for start in 0..=data.len().saturating_sub(len) {
                let slice = data[start..start + len].to_vec();
                let entry = counts.entry(slice).or_insert_with(WordStats::default);
                entry.count += 1;
            }
        }

        if counts.is_empty() {
            return;
        }

        let mut candidates: Vec<(Vec<u8>, WordStats)> = counts
            .into_iter()
            .filter_map(|(word, mut stats)| {
                let len = word.len();

                // Dynaaminen kynnys: ALHAISEMPI (löytää enemmän)
                let min_count = if len >= 15 {
                    2 // Pitkät lauseet (15+ tavua): 2 kertaa riittää
                } else if len >= 8 {
                    2 // Keskipitkät (8-14): 2 kertaa
                } else if len >= 5 {
                    3 // Lyhyet sanat (5-7): 3 kertaa
                } else {
                    4 // Hyvin lyhyet (3-4): 4 kertaa
                };

                if stats.count < min_count {
                    return None;
                }

                // Bonusta pidemmille sekvensseille
                let length_bonus = if len >= 20 {
                    2.0
                } else if len >= 10 {
                    1.5
                } else {
                    1.0
                };
                let estimated_gain =
                    (len.saturating_sub(3) as f64) * (stats.count as f64 - 1.0) * length_bonus;

                if estimated_gain <= 0.0 {
                    return None;
                }
                stats.estimated_gain = estimated_gain;
                Some((word, stats))
            })
            .collect();

        if candidates.is_empty() {
            return;
        }

        // Priorisoi pidemmät sekvenssit (lauseet) ja sitten estimated_gain
        candidates.sort_by(|a, b| {
            // 1. Vertaile pituutta (pidempi parempi)
            let len_cmp = b.0.len().cmp(&a.0.len());
            if len_cmp != Ordering::Equal {
                return len_cmp;
            }
            // 2. Jos samanpituisia, vertaile estimated_gain
            b.1.estimated_gain
                .partial_cmp(&a.1.estimated_gain)
                .unwrap_or(Ordering::Equal)
        });

        let top_candidates: Vec<_> = candidates
            .into_iter()
            .take(Self::DICTIONARY_CAPACITY)
            .collect();

        if !top_candidates.is_empty() {
            let longest = top_candidates
                .iter()
                .map(|(w, _)| w.len())
                .max()
                .unwrap_or(0);
            let avg_len =
                top_candidates.iter().map(|(w, _)| w.len()).sum::<usize>() / top_candidates.len();
            println!(
                "  📖 Sanakirja: {} ehdokasta, pisin: {} tavua, keskim: {} tavua",
                top_candidates.len(),
                longest,
                avg_len
            );
        }

        for (word, stats) in top_candidates {
            self.ensure_dictionary_word(word, stats);
        }
    }

    fn ensure_dictionary_word(&mut self, word: Vec<u8>, stats: WordStats) {
        if word.len() < Self::DICTIONARY_MIN_LEN {
            return;
        }

        if let Some(&id) = self.dictionary_lookup.get(&word) {
            if let Some(entry) = self.dictionary_words.get_mut(&id) {
                entry.frequency = stats.count;
                entry.recent_gain = stats.estimated_gain;
            }
            return;
        }

        if self.dictionary_words.len() >= Self::DICTIONARY_CAPACITY {
            self.evict_weak_dictionary_word();
            if self.dictionary_words.len() >= Self::DICTIONARY_CAPACITY {
                return;
            }
        }

        let word_id = self.next_word_id;
        self.next_word_id += 1;
        self.dictionary_lookup.insert(word.clone(), word_id);
        self.dictionary_words.insert(
            word_id,
            DictionaryWord {
                bytes: word.clone(),
                frequency: stats.count,
                recent_gain: stats.estimated_gain,
            },
        );

        let pattern_id = self.next_pattern_id;
        self.next_pattern_id += 1;
        let mut pattern = Pattern::new(
            pattern_id,
            Operator::Dictionary { word_id },
            self.cycle_count,
        );
        pattern.recent_gain = stats.estimated_gain;

        // Debug: näytä lisätty sana
        let word_preview = if word.len() <= 20 {
            String::from_utf8_lossy(&word).to_string()
        } else {
            format!("{}...", String::from_utf8_lossy(&word[..20]))
        };
        println!(
            "    + Sana #{}: \"{}\" ({} tavua, {} krt, gain: {:.1})",
            word_id,
            word_preview,
            word.len(),
            stats.count,
            stats.estimated_gain
        );

        self.known_patterns.push(pattern);
        self.forget_if_needed();
    }

    fn evict_weak_dictionary_word(&mut self) {
        let candidate = self
            .dictionary_words
            .iter()
            .min_by(|a, b| {
                a.1.recent_gain
                    .partial_cmp(&b.1.recent_gain)
                    .unwrap_or(Ordering::Equal)
            })
            .map(|(id, _)| *id);

        if let Some(word_id) = candidate {
            let remove_key = self
                .dictionary_lookup
                .iter()
                .find(|(_, id)| **id == word_id)
                .map(|(k, _)| k.clone());

            if let Some(key) = remove_key {
                self.dictionary_lookup.remove(&key);
            }

            self.dictionary_words.remove(&word_id);
            self.known_patterns.retain(|pattern| {
                !matches!(pattern.operator, Operator::Dictionary { word_id: id } if id == word_id)
            });
        }
    }

    fn find_dictionary_patch(&self, data: &[u8]) -> Option<Patch> {
        if self.dictionary_words.is_empty() || data.len() < Self::DICTIONARY_MIN_LEN {
            return None;
        }

        let mut words: Vec<(&u32, &DictionaryWord)> = self.dictionary_words.iter().collect();
        words.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

        let mut best: Option<(Patch, i32)> = None;
        for (word_id, word) in words.into_iter().take(Self::DICTIONARY_MATCH_LIMIT) {
            let len = word.len();
            if len > data.len() {
                continue;
            }

            for start in 0..=data.len().saturating_sub(len) {
                if &data[start..start + len] == word.bytes.as_slice() {
                    let saved = len as i32 - 3;
                    if saved <= 0 {
                        break;
                    }
                    let new_data = vec![
                        OP_DICT,
                        (*word_id & 0xFF) as u8,
                        ((*word_id >> 8) & 0xFF) as u8,
                    ];
                    let patch = Patch {
                        range: start..(start + len),
                        new_data,
                    };
                    match &best {
                        None => best = Some((patch, saved)),
                        Some((_, best_saved)) if saved > *best_saved => {
                            best = Some((patch, saved));
                        }
                        _ => {}
                    }
                    break;
                }
            }
        }

        best.map(|(patch, _)| patch)
    }

    /// REKURSIIVINEN PAKKAUS: Etsi toistuvia sekvenssejä JO PAKATUSTA datasta
    fn repack_compressed_data(
        &mut self,
        world: &mut World,
        evaluator: &crate::evaluator::Evaluator,
    ) {
        if world.data.len() < 10 {
            return;
        }

        println!("  🔄 MULTI-PASS REPACK: Etsitään meta-malleja syvällisesti...");

        let total_quota_budget = 500; // Kokonaisbudjetti kaikille kierroksille
        let mut total_used_quota = 0;
        let mut total_applied = 0;
        let mut pass_number = 1;

        // MULTI-PASS: Jatka repackaamista kunnes ei löydy enää mitään tai quota loppuu
        loop {
            if total_used_quota >= total_quota_budget {
                println!(
                    "    ⏱ Quota-budjetti käytetty ({} operaatiota)",
                    total_quota_budget
                );
                break;
            }

            println!(
                "    🔍 Pass #{}: Skannataan world.data ({} tavua)...",
                pass_number,
                world.data.len()
            );

            // Skannaa koko world.data ja etsi toistuvia byte-sekvenssejä
            let mut counts: HashMap<Vec<u8>, usize> = HashMap::new();

            // Etsi 4-48 tavun sekvenssejä
            for len in 4..=48 {
                if len > world.data.len() {
                    break;
                }
                for start in 0..=world.data.len().saturating_sub(len) {
                    let slice = world.data[start..start + len].to_vec();

                    // BONUS: Jos sekvenssi sisältää OP_DICT operaattoreita, se on meta-pattern!
                    let dict_ops_count = slice.iter().filter(|&&b| b == OP_DICT).count();
                    let bonus = if dict_ops_count > 0 { 2 } else { 1 };

                    *counts.entry(slice).or_insert(0) += bonus;
                }
            }

            // Löydä parhaat ehdokkaat
            let mut candidates: Vec<(Vec<u8>, usize)> = counts
                .into_iter()
                .filter(|(_seq, count)| *count >= 2)
                .collect();

            if candidates.is_empty() {
                println!(
                    "    ✓ Pass #{}: Ei löytynyt uusia toistuvia sekvenssejä - valmis!",
                    pass_number
                );
                break;
            }

            // Lajittele: pisin ensin, sitten useimmin toistuva
            candidates.sort_by(|a, b| {
                let len_cmp = b.0.len().cmp(&a.0.len());
                if len_cmp != Ordering::Equal {
                    return len_cmp;
                }
                b.1.cmp(&a.1)
            });

            let mut pass_applied = 0;
            let pass_quota_budget = (total_quota_budget - total_used_quota).min(200); // Max 200 per pass
            let mut pass_used_quota = 0;

            // Yritä käyttää 20 parasta sekvenssiä per pass
            for (seq, count) in candidates.into_iter().take(20) {
                if pass_used_quota >= pass_quota_budget {
                    break;
                }

                // Luo uusi Dictionary-entry
                let estimated_gain = (seq.len().saturating_sub(3) as f64) * (count as f64 - 1.0);
                if estimated_gain <= 0.0 {
                    continue;
                }

                let mut stats = WordStats::default();
                stats.count = count;
                stats.estimated_gain = estimated_gain;

                // Lisää sanakirjaan
                self.ensure_dictionary_word(seq.clone(), stats);

                // Sovella tätä mallia heti
                let cost_before = evaluator.calculate_total_cost(world);
                let mut changed = false;

                // Etsi ja korvaa KAIKKI esiintymät
                let mut i = 0;
                while i <= world.data.len().saturating_sub(seq.len()) {
                    if &world.data[i..i + seq.len()] == seq.as_slice() {
                        // Löytyi! Korvaa Dictionary-viittauksella
                        if let Some(&word_id) = self.dictionary_lookup.get(&seq) {
                            let new_data = vec![
                                OP_DICT,
                                (word_id & 0xFF) as u8,
                                ((word_id >> 8) & 0xFF) as u8,
                            ];

                            world
                                .data
                                .splice(i..i + seq.len(), new_data.iter().cloned());
                            changed = true;
                            pass_applied += 1;
                            i += 3; // Hyppää uuden viittauksen yli
                        } else {
                            i += 1;
                        }
                    } else {
                        i += 1;
                    }
                }

                if changed {
                    let cost_after = evaluator.calculate_total_cost(world);
                    let gain = evaluator.calculate_gain(cost_before, cost_after);
                    println!(
                        "      ✓ Meta-malli: {} tavua, {} krt → {} tavua säästetty",
                        seq.len(),
                        count,
                        gain
                    );

                    // Päivitä recent_gain jotta exploit käyttää tätä mallia
                    if let Some(&word_id) = self.dictionary_lookup.get(&seq) {
                        for pattern in &mut self.known_patterns {
                            if let Operator::Dictionary { word_id: pid } = pattern.operator {
                                if pid == word_id {
                                    pattern.recent_gain = gain as f64 * 10.0;
                                    break;
                                }
                            }
                        }
                    }
                }

                pass_used_quota += 10;
            }

            total_applied += pass_applied;
            total_used_quota += pass_used_quota;

            if pass_applied == 0 {
                println!(
                    "    ✓ Pass #{}: Ei sovellettu uusia malleja - valmis!",
                    pass_number
                );
                break;
            } else {
                println!(
                    "    ✓ Pass #{}: {} mallia sovellettu, jatketaan...",
                    pass_number, pass_applied
                );
                pass_number += 1;

                // Rajoita kierrosten määrää
                if pass_number > 10 {
                    println!("    ⚠ Maksimi 10 passia saavutettu");
                    break;
                }
            }
        }

        if total_applied > 0 {
            println!(
                "  ✅ Multi-pass repack valmis: {} mallia sovellettu {} passissa",
                total_applied,
                pass_number - 1
            );
        }
    }
}

#[derive(Clone)]
struct DictionaryWord {
    bytes: Vec<u8>,
    frequency: usize,
    recent_gain: f64,
}

impl DictionaryWord {
    fn len(&self) -> usize {
        self.bytes.len()
    }
}

#[derive(Default, Clone)]
struct WordStats {
    count: usize,
    estimated_gain: f64,
}

#[derive(Clone)]
#[allow(dead_code)]
struct GrammarRule {
    #[allow(dead_code)]
    id: u32,
    #[allow(dead_code)]
    sequence: Vec<OperatorKey>,
    #[allow(dead_code)]
    encoded_len: usize,
}

#[derive(Hash, Eq, PartialEq, Clone)]
#[allow(dead_code)]
enum OperatorKey {
    RunLength(u8, usize),
    GeneralizedRunLength(usize),
    BackRef(usize, usize),
    BackRefRange {
        min_distance: usize,
        max_distance: usize,
        len: usize,
    },
    Delta(u8, i8, usize),
    Xor(Vec<u8>, u8, usize),
    Dictionary(u32),
    Grammar(u32),
}
