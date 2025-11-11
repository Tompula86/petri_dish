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
}

impl Solver {
    pub fn new(processing_quota: u32) -> Self {
        Solver {
            processing_quota,
            known_patterns: Vec::new(),
            next_pattern_id: 0,
            stats: Stats::new(),
        }
    }

    /// Pääsilmukka: Solver operoi Worldissä
    /// VAIHE 5: exploit → explore → opi + tilastot
    pub fn live(&mut self, world: &mut World, evaluator: &crate::evaluator::Evaluator) {
        // Nollaa syklin tilastot
        self.stats.reset_cycle();
        
        // 1) EXPLOIT: Kokeile ensin tunnettuja malleja
        if let Some((patch, pattern_id)) = self.exploit(world) {
            let cost_before = evaluator.calculate_total_cost(world);
            let original_data = world.get_data_in_range(patch.range.clone());
            
            // Quota-kustannus: exploit on nopea (1 quota per yritys)
            let quota_cost = 1;
            
            world.apply_patch(&patch);
            let cost_after = evaluator.calculate_total_cost(world);
            let gain = evaluator.calculate_gain(cost_before, cost_after);
            
            if gain > 0 {
                // Päivitä tilastot
                self.stats.record_exploit(quota_cost, gain);
                
                // Päivitä mallin tilastot
                if let Some(pattern) = self.known_patterns.iter_mut().find(|p| p.id == pattern_id) {
                    pattern.record_usage(gain);
                    println!("  ✓ Exploit onnistui (pattern #{}, säästö: {} tavua, käytetty {} kertaa)", 
                             pattern_id, gain, pattern.usage_count);
                }
                
                // Päivitä kustannuskomponentit
                self.update_cost_breakdown(world, evaluator);
                return; // Onnistui, ei tarvitse explore
            } else {
                world.rollback(&patch, original_data);
                self.stats.record_exploit(quota_cost, 0); // Epäonnistui
            }
        }

        // 2) EXPLORE: Etsi uusia malleja
        if let Some(patch) = self.explore(world) {
            let cost_before = evaluator.calculate_total_cost(world);
            let original_data = world.get_data_in_range(patch.range.clone());
            
            // Quota-kustannus: explore on kalliimpi (10 quota per yritys)
            let quota_cost = 10;
            
            // Tunnista operaattori patchistä
            let operator = self.identify_operator(&patch);
            
            world.apply_patch(&patch);
            let cost_after = evaluator.calculate_total_cost(world);
            let gain = evaluator.calculate_gain(cost_before, cost_after);
            
            if gain <= 0 {
                world.rollback(&patch, original_data);
                self.stats.record_explore(quota_cost, 0); // Epäonnistui
                println!("  ✗ Explore: Malli hylätty (tappio: {} tavua)", -gain);
            } else {
                // Päivitä tilastot
                self.stats.record_explore(quota_cost, gain);
                
                // 3) OPI: Lisää PatternBankiin
                let mut pattern = Pattern::new(self.next_pattern_id, operator);
                pattern.record_usage(gain);
                let pattern_id = self.next_pattern_id;
                self.next_pattern_id += 1;
                self.known_patterns.push(pattern);
                println!("  ✓ Explore: Uusi malli #{} löydetty (säästö: {} tavua)", pattern_id, gain);
                println!("  📚 PatternBank: {} mallia muistissa", self.known_patterns.len());
                
                // Päivitä kustannuskomponentit
                self.update_cost_breakdown(world, evaluator);
            }
        } else {
            // Ei löytynyt mitään
            self.update_cost_breakdown(world, evaluator);
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
        } else {
            // Oletus: RunLength jos tuntematon
            Operator::RunLength(0, 0)
        }
    }

    /// Exploit: Käytä tunnettuja malleja
    /// Palauttaa: (Patch, pattern_id) jos löytyi sovellettava malli
    fn exploit(&self, world: &World) -> Option<(Patch, u32)> {
        // Käy läpi kaikki tunnetut mallit
        for pattern in &self.known_patterns {
            match &pattern.operator {
                Operator::RunLength(byte, min_count) => {
                    // Etsi tämän mallin mukaisia toistoja
                    if let Some(patch) = self.find_run_length(world, *byte, *min_count) {
                        return Some((patch, pattern.id));
                    }
                }
            }
        }
        None
    }

    /// Explore: Etsi uusia malleja Worldistä
    /// Tällä hetkellä: etsii RunLength-malleja (5+ samaa peräkkäistä tavua)
    fn explore(&self, world: &World) -> Option<Patch> {
        const MIN_RUN_LENGTH: usize = 5;
        self.find_any_run_length(world, MIN_RUN_LENGTH)
    }

    /// Apufunktio: etsi mitä tahansa RunLength-mallia
    fn find_any_run_length(&self, world: &World, min_count: usize) -> Option<Patch> {
        let data = &world.data;
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
    fn find_run_length(&self, world: &World, target_byte: u8, min_count: usize) -> Option<Patch> {
        let data = &world.data;
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
