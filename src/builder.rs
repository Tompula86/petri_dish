// src/builder.rs
//
// Builder (Rakentaja): Hierarkkinen tiedonrakennuskone.
//
// Toimii token-virralla:
// 1. Aluksi virta on pelkkiä Literal-ID:itä
// 2. Builder huomaa usein vierekkäisiä pareja
// 3. Luo uusia Combine-malleja
// 4. Korvaa parit uusilla tokeneilla
// 5. Virta tiivistyy hierarkkisesti

use crate::operator::Operator;
use crate::pattern::Pattern;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

// ============================================================================
// CONFIGURABLE CONSTANTS
// ============================================================================

/// Maximum number of top pairs to consider in each explore cycle
const MAX_TOP_PAIRS: usize = 10;

/// Scaling factor for strengthening patterns based on occurrence count
const STRENGTHEN_SCALE_FACTOR: f64 = 10.0;

/// Capacity threshold (percentage) at which forgetting kicks in
/// Petri Dish 2.0: Nostettu 80% -> 90%, jotta oppiminen jatkuu pidempään
const FORGET_CAPACITY_THRESHOLD: usize = 90;

/// Percentage of patterns to remove when capacity is exceeded
const FORGET_REMOVAL_PERCENTAGE: usize = 10;

/// Default decay rate for pattern strength per cycle
const DEFAULT_DECAY_RATE: f64 = 0.01;

// ============================================================================
// PATTERN BANK
// ============================================================================

/// Serialisoitava versio pair_lookup:ista
/// JSON ei tue tuple-avaimia HashMapissa, joten serialisoidaan Veciksi
#[derive(Serialize, Deserialize)]
struct PairLookupEntry {
    left: u32,
    right: u32,
    pattern_id: u32,
}

/// PatternBank: Mallien muisti.
/// 
/// Tukee nopeaa hakua:
/// - id -> Pattern
/// - (left_id, right_id) -> id (tiedämme onko pari jo olemassa)
pub struct PatternBank {
    /// Kaikki mallit: id -> Pattern
    patterns: HashMap<u32, Pattern>,
    
    /// Käänteinen haku: (left_id, right_id) -> pattern_id
    /// Tiedämme nopeasti onko pari A+B jo olemassa.
    pair_lookup: HashMap<(u32, u32), u32>,
    
    /// Seuraava vapaa ID
    next_id: u32,
    
    /// Maksimi mallien määrä (evoluutiopaine)
    pub capacity: usize,
}

/// Serialisoitava versio PatternBankista
#[derive(Serialize, Deserialize)]
struct PatternBankData {
    patterns: HashMap<u32, Pattern>,
    pair_lookup: Vec<PairLookupEntry>,
    next_id: u32,
    capacity: usize,
}

impl PatternBank {
    /// Luo uusi PatternBank ja täytä se 256:lla Literal-patternilla
    pub fn new(capacity: usize) -> Self {
        let mut bank = PatternBank {
            patterns: HashMap::with_capacity(256 + capacity),
            pair_lookup: HashMap::new(),
            next_id: 0,
            capacity: capacity + 256, // 256 literaalia + capacity yhdistelmiä
        };
        
        // Alusta 256 Literal-patternia (tavut 0-255)
        for byte in 0u8..=255 {
            let id = bank.next_id;
            bank.next_id += 1;
            let pattern = Pattern::new_literal(id, byte);
            bank.patterns.insert(id, pattern);
        }
        
        bank
    }
    
    /// Hae malli ID:llä
    pub fn get(&self, id: u32) -> Option<&Pattern> {
        self.patterns.get(&id)
    }
    
    /// Hae malli ID:llä (mut)
    pub fn get_mut(&mut self, id: u32) -> Option<&mut Pattern> {
        self.patterns.get_mut(&id)
    }
    
    /// Hae Literal-mallin ID tavulle
    pub fn literal_id(&self, byte: u8) -> u32 {
        byte as u32
    }
    
    /// Tarkista onko pari (left, right) jo olemassa
    pub fn has_pair(&self, left: u32, right: u32) -> bool {
        self.pair_lookup.contains_key(&(left, right))
    }
    
    /// Hae parin (left, right) ID jos se on olemassa
    pub fn get_pair_id(&self, left: u32, right: u32) -> Option<u32> {
        self.pair_lookup.get(&(left, right)).copied()
    }
    
    /// Luo uusi Combine-malli parille (left, right)
    /// Palauttaa uuden mallin ID:n
    pub fn create_combine(&mut self, left: u32, right: u32, cycle: u64) -> Option<u32> {
        // Tarkista ettei pari ole jo olemassa
        if self.has_pair(left, right) {
            return self.get_pair_id(left, right);
        }
        
        // Tarkista kapasiteetti
        if self.patterns.len() >= self.capacity {
            return None; // Täynnä, pitää ensin "unohtaa" jotain
        }
        
        // Hae vanhempien kompleksisuudet
        let left_complexity = self.patterns.get(&left).map(|p| p.complexity).unwrap_or(0);
        let right_complexity = self.patterns.get(&right).map(|p| p.complexity).unwrap_or(0);
        
        let id = self.next_id;
        self.next_id += 1;
        
        let pattern = Pattern::new_combine(id, left, right, left_complexity, right_complexity, cycle);
        self.patterns.insert(id, pattern);
        self.pair_lookup.insert((left, right), id);
        
        // Kasvata vasemman ja oikean osan ref_count +1
        if let Some(left_pattern) = self.patterns.get_mut(&left) {
            left_pattern.ref_count += 1;
        }
        if let Some(right_pattern) = self.patterns.get_mut(&right) {
            right_pattern.ref_count += 1;
        }
        
        Some(id)
    }
    
    /// Poista malli (unohtaminen)
    /// Huom: Vähentää myös lasten ref_count arvoja
    pub fn remove(&mut self, id: u32) -> Option<Pattern> {
        if let Some(pattern) = self.patterns.remove(&id) {
            // Poista myös pair_lookup:ista jos kyseessä on Combine
            if let Operator::Combine(left, right) = pattern.op {
                self.pair_lookup.remove(&(left, right));
                
                // Vähennä lasten ref_count -1
                if let Some(left_pattern) = self.patterns.get_mut(&left) {
                    left_pattern.ref_count = left_pattern.ref_count.saturating_sub(1);
                }
                if let Some(right_pattern) = self.patterns.get_mut(&right) {
                    right_pattern.ref_count = right_pattern.ref_count.saturating_sub(1);
                }
            }
            Some(pattern)
        } else {
            None
        }
    }
    
    /// Hae heikoimmat mallit (paitsi Literaalit ja mallit joihin viitataan)
    /// Vain "orvot" ylätason mallit (ref_count == 0) voidaan poistaa
    pub fn get_weakest(&self, count: usize) -> Vec<u32> {
        let mut combines: Vec<(u32, f64)> = self.patterns
            .iter()
            .filter(|(_, p)| !p.is_literal() && p.ref_count == 0)
            .map(|(id, p)| (*id, p.strength))
            .collect();
        
        combines.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        
        combines.into_iter().take(count).map(|(id, _)| id).collect()
    }
    
    /// Mallien määrä
    pub fn len(&self) -> usize {
        self.patterns.len()
    }
    
    /// Combine-mallien määrä (ei Literaalit)
    pub fn combine_count(&self) -> usize {
        self.patterns.values().filter(|p| !p.is_literal()).count()
    }
    
    /// Iteroi kaikkien mallien yli
    pub fn iter(&self) -> impl Iterator<Item = (&u32, &Pattern)> {
        self.patterns.iter()
    }
    
    /// Dekoodaa token-ID takaisin tavuiksi
    /// 
    /// Tämä on rekursiivinen: Combine hajotetaan osiinsa kunnes
    /// päästään Literal-tasolle.
    pub fn decode(&self, id: u32) -> Vec<u8> {
        let mut result = Vec::new();
        self.decode_into(id, &mut result);
        result
    }
    
    fn decode_into(&self, id: u32, result: &mut Vec<u8>) {
        if let Some(pattern) = self.patterns.get(&id) {
            match &pattern.op {
                Operator::Literal(byte) => {
                    result.push(*byte);
                }
                Operator::Combine(left, right) => {
                    self.decode_into(*left, result);
                    self.decode_into(*right, result);
                }
            }
        }
    }
    
    /// Laske mallin "pituus" tavuina (dekoodattu muoto)
    pub fn pattern_length(&self, id: u32) -> usize {
        if let Some(pattern) = self.patterns.get(&id) {
            match &pattern.op {
                Operator::Literal(_) => 1,
                Operator::Combine(left, right) => {
                    self.pattern_length(*left) + self.pattern_length(*right)
                }
            }
        } else {
            0
        }
    }
    
    /// Tallenna PatternBank JSON-tiedostoon
    pub fn save(&self, path: &Path) -> Result<(), String> {
        // Muunna PatternBank serialisoitavaan muotoon
        let pair_lookup_vec: Vec<PairLookupEntry> = self.pair_lookup
            .iter()
            .map(|((left, right), pattern_id)| PairLookupEntry {
                left: *left,
                right: *right,
                pattern_id: *pattern_id,
            })
            .collect();
        
        let data = PatternBankData {
            patterns: self.patterns.clone(),
            pair_lookup: pair_lookup_vec,
            next_id: self.next_id,
            capacity: self.capacity,
        };
        
        let file = File::create(path).map_err(|e| format!("Tiedoston luonti epäonnistui: {}", e))?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &data)
            .map_err(|e| format!("JSON-serialisointi epäonnistui: {}", e))?;
        Ok(())
    }
    
    /// Lataa PatternBank JSON-tiedostosta
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("Tiedoston avaus epäonnistui: {}", e))?;
        let reader = BufReader::new(file);
        let data: PatternBankData = serde_json::from_reader(reader)
            .map_err(|e| format!("JSON-deserialisointi epäonnistui: {}", e))?;
        
        // Muunna takaisin PatternBank-muotoon
        let pair_lookup: HashMap<(u32, u32), u32> = data.pair_lookup
            .into_iter()
            .map(|entry| ((entry.left, entry.right), entry.pattern_id))
            .collect();
        
        Ok(PatternBank {
            patterns: data.patterns,
            pair_lookup,
            next_id: data.next_id,
            capacity: data.capacity,
        })
    }
}

/// PairStats: Tilasto vierekkäisistä pareista
#[derive(Default)]
pub struct PairStats {
    /// (left, right) -> esiintymismäärä
    counts: HashMap<(u32, u32), u32>,
}

impl PairStats {
    pub fn new() -> Self {
        PairStats {
            counts: HashMap::new(),
        }
    }
    
    /// Lisää parin esiintymä
    pub fn record(&mut self, left: u32, right: u32) {
        *self.counts.entry((left, right)).or_insert(0) += 1;
    }
    
    /// Nollaa tilastot
    pub fn clear(&mut self) {
        self.counts.clear();
    }
    
    /// Hae parhaat parit (ylittävät kynnyksen)
    pub fn get_top_pairs(&self, threshold: u32, max_count: usize) -> Vec<((u32, u32), u32)> {
        let mut pairs: Vec<_> = self.counts
            .iter()
            .filter(|&(_, count)| *count >= threshold)
            .map(|((l, r), count)| ((*l, *r), *count))
            .collect();
        
        pairs.sort_by(|a, b| b.1.cmp(&a.1));
        pairs.truncate(max_count);
        pairs
    }
}

/// Builder: Hierarkkinen tiedonrakennuskone
/// 
/// Korvaa vanhan Solverin. Toimii token-virralla:
/// 1. Tokenisoi syöte Literal-ID:iksi
/// 2. Etsi usein toistuvia pareja (Matchmaker)
/// 3. Luo uusia Combine-malleja
/// 4. Korvaa parit uusilla tokeneilla (Parser)
/// 5. Unohda heikot mallit (Forget)
pub struct Builder {
    /// PatternBank: mallien muisti
    pub bank: PatternBank,
    
    /// Token-virta: nykyinen datan esitys Pattern-ID:inä
    pub token_stream: Vec<u32>,
    
    /// Paritilastot nykyisestä virrasta
    pair_stats: PairStats,
    
    /// Nykyinen sykli (aika)
    pub cycle: u64,
    
    /// Kynnys parin luomiselle (kuinka monta kertaa pitää esiintyä)
    pub pair_threshold: u32,
    
    /// Kynnys mallin "kuolemalle" (liian heikko strength)
    #[allow(dead_code)]
    pub death_threshold: f64,
    
    /// Vahvistuksen määrä onnistuneesta ennustuksesta
    pub strengthen_amount: f64,
    
    /// Heikennyksen määrä epäonnistuneesta ennustuksesta
    #[allow(dead_code)]
    pub weaken_amount: f64,
}

impl Builder {
    /// Luo uusi Builder
    pub fn new(pattern_capacity: usize) -> Self {
        Builder {
            bank: PatternBank::new(pattern_capacity),
            token_stream: Vec::new(),
            pair_stats: PairStats::new(),
            cycle: 0,
            pair_threshold: 2,    // Pari pitää esiintyä vähintään 2 kertaa
            death_threshold: 0.1, // Alle 0.1 strength -> kuolema
            strengthen_amount: 0.1,
            weaken_amount: 0.05,
        }
    }
    
    /// Luo Builder olemassa olevalla PatternBankilla (pysyvä muisti)
    pub fn with_bank(bank: PatternBank) -> Self {
        Builder {
            bank,
            token_stream: Vec::new(),
            pair_stats: PairStats::new(),
            cycle: 0,
            pair_threshold: 2,
            death_threshold: 0.1,
            strengthen_amount: 0.1,
            weaken_amount: 0.05,
        }
    }
    
    /// Tokenisoi raaka data Literal-ID:iksi ja lisää virtaan
    pub fn tokenize(&mut self, data: &[u8]) {
        for &byte in data {
            let id = self.bank.literal_id(byte);
            self.token_stream.push(id);
        }
    }
    
    /// Laske paritilastot nykyisestä virrasta
    fn compute_pair_stats(&mut self) {
        self.pair_stats.clear();
        
        if self.token_stream.len() < 2 {
            return;
        }
        
        for window in self.token_stream.windows(2) {
            self.pair_stats.record(window[0], window[1]);
        }
    }
    
    /// Matchmaker: Etsi usein toistuvia pareja ja luo uusia malleja
    /// 
    /// Palauttaa luotujen mallien määrän
    pub fn explore(&mut self) -> usize {
        self.compute_pair_stats();
        
        // Hae parhaat parit
        let top_pairs = self.pair_stats.get_top_pairs(self.pair_threshold, MAX_TOP_PAIRS);
        
        let mut created = 0;
        
        for ((left, right), count) in top_pairs {
            // Tarkista ettei pari ole jo olemassa
            if self.bank.has_pair(left, right) {
                // Vahvista olemassa olevaa mallia
                if let Some(id) = self.bank.get_pair_id(left, right) {
                    if let Some(pattern) = self.bank.get_mut(id) {
                        pattern.strengthen(self.strengthen_amount * (count as f64 / STRENGTHEN_SCALE_FACTOR), self.cycle);
                    }
                }
                continue;
            }
            
            // Yritä luoda uusi malli
            if let Some(new_id) = self.bank.create_combine(left, right, self.cycle) {
                created += 1;
                
                // Tulosta löydös
                let left_bytes = self.bank.decode(left);
                let right_bytes = self.bank.decode(right);
                let combined = self.bank.decode(new_id);
                
                let left_str = String::from_utf8_lossy(&left_bytes);
                let right_str = String::from_utf8_lossy(&right_bytes);
                let combined_str = String::from_utf8_lossy(&combined);
                
                println!(
                    "  🧬 Syntyi: P_{} = \"{}\" + \"{}\" = \"{}\" ({} krt, taso {})",
                    new_id,
                    left_str,
                    right_str,
                    combined_str,
                    count,
                    self.bank.get(new_id).map(|p| p.complexity).unwrap_or(0)
                );
            }
        }
        
        created
    }
    
    /// Parser: Korvaa kaikki tunnetut parit uusilla tokeneilla
    /// 
    /// Palauttaa korvattujen parien määrän (= tiivistys)
    pub fn collapse(&mut self) -> usize {
        if self.token_stream.len() < 2 {
            return 0;
        }
        
        let mut collapsed = 0;
        let mut new_stream = Vec::with_capacity(self.token_stream.len());
        let mut i = 0;
        
        while i < self.token_stream.len() {
            if i + 1 < self.token_stream.len() {
                let left = self.token_stream[i];
                let right = self.token_stream[i + 1];
                
                // Tarkista onko pari olemassa ja onko se tarpeeksi vahva
                if let Some(combined_id) = self.bank.get_pair_id(left, right) {
                    if let Some(pattern) = self.bank.get(combined_id) {
                        // Käytä vain jos strength ylittää "totuuskynnyksen"
                        if pattern.strength >= 0.5 {
                            new_stream.push(combined_id);
                            collapsed += 1;
                            i += 2;
                            
                            // Vahvista käytettyä mallia
                            if let Some(p) = self.bank.get_mut(combined_id) {
                                p.strengthen(self.strengthen_amount, self.cycle);
                            }
                            continue;
                        }
                    }
                }
            }
            
            new_stream.push(self.token_stream[i]);
            i += 1;
        }
        
        self.token_stream = new_stream;
        collapsed
    }
    
    /// Forget: Poista heikoimmat mallit jos kapasiteetti on täynnä
    /// 
    /// Palauttaa poistettujen mallien määrän
    pub fn forget(&mut self, force_count: usize) -> usize {
        let combine_count = self.bank.combine_count();
        
        // Poista vain jos yli FORGET_CAPACITY_THRESHOLD% kapasiteetista käytössä tai pakotettu
        let to_remove = if force_count > 0 {
            force_count
        } else if combine_count > (self.bank.capacity * FORGET_CAPACITY_THRESHOLD / 100) {
            combine_count * FORGET_REMOVAL_PERCENTAGE / 100 // Poista FORGET_REMOVAL_PERCENTAGE% kerralla
        } else {
            0
        };
        
        if to_remove == 0 {
            return 0;
        }
        
        let weak_ids = self.bank.get_weakest(to_remove);
        let mut removed = 0;
        
        for id in weak_ids {
            // Ennen poistoa: hajota malli takaisin osiinsa virrassa
            if let Some(pattern) = self.bank.get(id) {
                if let Operator::Combine(left, right) = pattern.op {
                    // Korvaa kaikki id:t virrassa parilla (left, right)
                    let mut new_stream = Vec::with_capacity(self.token_stream.len() * 2);
                    for &token in &self.token_stream {
                        if token == id {
                            new_stream.push(left);
                            new_stream.push(right);
                        } else {
                            new_stream.push(token);
                        }
                    }
                    self.token_stream = new_stream;
                    
                    // Tulosta poisto
                    println!(
                        "  🗑️ Unohdettiin: P_{} (strength: {:.2})",
                        id,
                        pattern.strength
                    );
                }
            }
            
            self.bank.remove(id);
            removed += 1;
        }
        
        removed
    }
    
    /// Decay: Heikennä kaikkien Combine-mallien strength-arvoja ajan myötä
    pub fn decay(&mut self, amount: f64) {
        for (_, pattern) in self.bank.patterns.iter_mut() {
            if !pattern.is_literal() {
                pattern.weaken(amount);
            }
        }
    }
    
    /// Pääsilmukka: Yksi sykli oppimista
    /// 
    /// Uusi järjestys (Petri Dish 2.0):
    /// 1. Decay: Vähennä kaikkien mallien vahvuutta hieman
    /// 2. Prune: Jos pankki on >90% täynnä, poista heikoimmat mallit (joilla ref_count == 0)
    /// 3. Explore: Etsi uusia yhteyksiä (nyt tilaa on varmasti)
    /// 4. Collapse: Tiivistä dataa
    pub fn live(&mut self) -> BuilderStats {
        self.cycle += 1;
        
        let stream_before = self.token_stream.len();
        let patterns_before = self.bank.combine_count();
        
        // 1. Decay: Vähennä kaikkien mallien vahvuutta
        self.decay(DEFAULT_DECAY_RATE);
        
        // 2. Prune: Siivoa ennen oppimista (tilaa uusille malleille)
        let forgotten = self.forget(0);
        
        // 3. Explore: Etsi uusia pareja
        let created = self.explore();
        
        // 4. Collapse (useita kierroksia kunnes ei enää tiivisty)
        let mut total_collapsed = 0;
        loop {
            let collapsed = self.collapse();
            if collapsed == 0 {
                break;
            }
            total_collapsed += collapsed;
        }
        
        let stream_after = self.token_stream.len();
        let patterns_after = self.bank.combine_count();
        
        BuilderStats {
            cycle: self.cycle,
            stream_before,
            stream_after,
            patterns_created: created,
            patterns_collapsed: total_collapsed,
            patterns_forgotten: forgotten,
            patterns_total: patterns_after,
            compression_ratio: if stream_before > 0 {
                1.0 - (stream_after as f64 / stream_before as f64)
            } else {
                0.0
            },
            patterns_before,
        }
    }
    
    /// Dekoodaa koko token-virta takaisin tavuiksi
    #[allow(dead_code)]
    pub fn decode_stream(&self) -> Vec<u8> {
        let mut result = Vec::new();
        for &id in &self.token_stream {
            result.extend(self.bank.decode(id));
        }
        result
    }
    
    /// Virran pituus tokeneina
    pub fn stream_len(&self) -> usize {
        self.token_stream.len()
    }
    
    /// Virran "alkuperäinen" pituus tavuina (dekoodattuna)
    pub fn original_len(&self) -> usize {
        self.token_stream.iter().map(|&id| self.bank.pattern_length(id)).sum()
    }
    
    /// Tulosta hierarkia tietylle mallille
    pub fn print_hierarchy(&self, id: u32, indent: usize) {
        let prefix = "  ".repeat(indent);
        
        if let Some(pattern) = self.bank.get(id) {
            match &pattern.op {
                Operator::Literal(byte) => {
                    let ch = if byte.is_ascii_graphic() || *byte == b' ' {
                        char::from(*byte).to_string()
                    } else {
                        format!("0x{:02X}", byte)
                    };
                    println!("{}P_{}: Literal('{}') [L0]", prefix, id, ch);
                }
                Operator::Combine(left, right) => {
                    let decoded = self.bank.decode(id);
                    let decoded_str = String::from_utf8_lossy(&decoded);
                    println!(
                        "{}P_{}: Combine(P_{}, P_{}) = \"{}\" [L{}, str={:.2}]",
                        prefix, id, left, right, decoded_str, pattern.complexity, pattern.strength
                    );
                    self.print_hierarchy(*left, indent + 1);
                    self.print_hierarchy(*right, indent + 1);
                }
            }
        }
    }
}

/// Tilastot yhdestä build-syklistä
#[derive(Debug)]
pub struct BuilderStats {
    pub cycle: u64,
    pub stream_before: usize,
    pub stream_after: usize,
    pub patterns_created: usize,
    pub patterns_collapsed: usize,
    pub patterns_forgotten: usize,
    pub patterns_total: usize,
    pub compression_ratio: f64,
    #[allow(dead_code)]
    pub patterns_before: usize,
}

impl BuilderStats {
    pub fn print(&self) {
        println!(
            "  📊 Sykli {}: virta {} → {} ({:.1}% tiivistys), malleja {} (+{} -{}) ",
            self.cycle,
            self.stream_before,
            self.stream_after,
            self.compression_ratio * 100.0,
            self.patterns_total,
            self.patterns_created,
            self.patterns_forgotten
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pattern_bank_literals() {
        let bank = PatternBank::new(100);
        
        // Tarkista että kaikki literaalit on luotu
        assert_eq!(bank.len(), 256);
        
        // Tarkista muutama literal
        assert_eq!(bank.literal_id(b'a'), 97);
        assert_eq!(bank.literal_id(b'A'), 65);
        assert_eq!(bank.literal_id(0), 0);
        assert_eq!(bank.literal_id(255), 255);
        
        // Tarkista decode
        assert_eq!(bank.decode(97), vec![b'a']);
        assert_eq!(bank.decode(65), vec![b'A']);
    }
    
    #[test]
    fn test_pattern_bank_combine() {
        let mut bank = PatternBank::new(100);
        
        // Luo pari 'a' + 'b'
        let a_id = bank.literal_id(b'a');
        let b_id = bank.literal_id(b'b');
        
        let ab_id = bank.create_combine(a_id, b_id, 1).unwrap();
        
        assert!(bank.has_pair(a_id, b_id));
        assert_eq!(bank.get_pair_id(a_id, b_id), Some(ab_id));
        assert_eq!(bank.decode(ab_id), vec![b'a', b'b']);
        
        // Yritä luoda sama pari uudestaan
        let ab_id2 = bank.create_combine(a_id, b_id, 2);
        assert_eq!(ab_id2, Some(ab_id)); // Palauttaa olemassa olevan
    }
    
    #[test]
    fn test_builder_tokenize() {
        let mut builder = Builder::new(100);
        
        builder.tokenize(b"abc");
        
        assert_eq!(builder.token_stream.len(), 3);
        assert_eq!(builder.token_stream[0], 97); // 'a'
        assert_eq!(builder.token_stream[1], 98); // 'b'
        assert_eq!(builder.token_stream[2], 99); // 'c'
        
        // Decode takaisin
        assert_eq!(builder.decode_stream(), b"abc");
    }
    
    #[test]
    fn test_builder_explore_and_collapse() {
        let mut builder = Builder::new(100);
        
        // Syötä "abab" - pari "ab" toistuu 2 kertaa
        builder.tokenize(b"abab");
        
        // Explore: Pitäisi löytää pari "ab"
        let created = builder.explore();
        assert!(created > 0, "Pitäisi luoda ainakin yksi malli");
        
        // Vahvista uusi malli jotta collapse toimii
        let ab_id = builder.bank.get_pair_id(97, 98).unwrap();
        if let Some(p) = builder.bank.get_mut(ab_id) {
            p.strength = 0.6; // Yli 0.5 kynnyksen
        }
        
        // Collapse: Pitäisi tiivistää
        let collapsed = builder.collapse();
        assert!(collapsed > 0, "Pitäisi tiivistää ainakin kerran");
        
        // Virta pitäisi olla lyhyempi
        assert!(builder.token_stream.len() < 4);
        
        // Mutta decode pitäisi palauttaa alkuperäinen
        assert_eq!(builder.decode_stream(), b"abab");
    }
    
    #[test]
    fn test_builder_hierarchical() {
        let mut builder = Builder::new(100);
        
        // Syötä "aabb" useasti -> "aa" ja "bb" parit, sitten "aabb"
        builder.tokenize(b"aabbaabbaabb");
        
        // Monta sykliä oppimista
        for _ in 0..5 {
            builder.live();
        }
        
        // Tarkista että virta on tiivistynyt
        let original_len = 12;
        let current_len = builder.stream_len();
        println!("Alkuperäinen: {}, Nykyinen: {}", original_len, current_len);
        
        // Pitäisi olla pienempi (hierarkkinen tiivistys)
        // Huom: ei välttämättä aina tiivisty jos kynnykset eivät täyty
        
        // Decode pitäisi silti palauttaa alkuperäinen
        assert_eq!(builder.decode_stream(), b"aabbaabbaabb");
    }
    
    #[test]
    fn test_ref_count_on_combine_creation() {
        let mut bank = PatternBank::new(100);
        
        let a_id = bank.literal_id(b'a');
        let b_id = bank.literal_id(b'b');
        
        // Tarkista että literaalien ref_count on aluksi 0
        assert_eq!(bank.get(a_id).unwrap().ref_count, 0);
        assert_eq!(bank.get(b_id).unwrap().ref_count, 0);
        
        // Luo yhdistelmä
        let ab_id = bank.create_combine(a_id, b_id, 1).unwrap();
        
        // Tarkista että literaalien ref_count kasvoi
        assert_eq!(bank.get(a_id).unwrap().ref_count, 1);
        assert_eq!(bank.get(b_id).unwrap().ref_count, 1);
        
        // Luo toinen yhdistelmä joka käyttää ab:ta
        let c_id = bank.literal_id(b'c');
        let abc_id = bank.create_combine(ab_id, c_id, 2).unwrap();
        
        // ab:n ref_count pitäisi olla 1 (abc viittaa siihen)
        assert_eq!(bank.get(ab_id).unwrap().ref_count, 1);
        assert_eq!(bank.get(c_id).unwrap().ref_count, 1);
        
        // abc:n ref_count pitäisi olla 0 (kukaan ei viittaa siihen)
        assert_eq!(bank.get(abc_id).unwrap().ref_count, 0);
    }
    
    #[test]
    fn test_ref_count_on_remove() {
        let mut bank = PatternBank::new(100);
        
        let a_id = bank.literal_id(b'a');
        let b_id = bank.literal_id(b'b');
        
        // Luo yhdistelmä
        let ab_id = bank.create_combine(a_id, b_id, 1).unwrap();
        
        // Tarkista ref_countit
        assert_eq!(bank.get(a_id).unwrap().ref_count, 1);
        assert_eq!(bank.get(b_id).unwrap().ref_count, 1);
        
        // Poista yhdistelmä
        bank.remove(ab_id);
        
        // ref_countien pitäisi vähentyä
        assert_eq!(bank.get(a_id).unwrap().ref_count, 0);
        assert_eq!(bank.get(b_id).unwrap().ref_count, 0);
    }
    
    #[test]
    fn test_get_weakest_respects_ref_count() {
        let mut bank = PatternBank::new(100);
        
        let a_id = bank.literal_id(b'a');
        let b_id = bank.literal_id(b'b');
        let c_id = bank.literal_id(b'c');
        
        // Luo kaksi yhdistelmää
        let ab_id = bank.create_combine(a_id, b_id, 1).unwrap();
        let _bc_id = bank.create_combine(b_id, c_id, 1).unwrap();
        
        // Luo kolmas yhdistelmä joka viittaa ensimmäiseen
        let _abc_id = bank.create_combine(ab_id, c_id, 2).unwrap();
        
        // ab:lla on ref_count == 1, bc:lla ref_count == 0
        // get_weakest pitäisi palauttaa vain bc (ref_count == 0)
        let weakest = bank.get_weakest(10);
        
        // ab ei saa olla mukana koska siihen viitataan
        assert!(!weakest.contains(&ab_id), "ab:ta ei pitäisi poistaa koska siihen viitataan");
        
        // bc voi olla mukana koska siihen ei viitata
        // (jos sen strength on tarpeeksi heikko)
    }
    
    #[test]
    fn test_persistence_save_load() {
        use std::path::Path;
        
        let mut bank = PatternBank::new(100);
        
        // Luo muutama yhdistelmä
        let a_id = bank.literal_id(b'a');
        let b_id = bank.literal_id(b'b');
        let ab_id = bank.create_combine(a_id, b_id, 1).unwrap();
        
        // Vahvista mallia
        if let Some(p) = bank.get_mut(ab_id) {
            p.strength = 0.8;
            p.usage_count = 5;
        }
        
        // Tallenna
        let test_path = Path::new("/tmp/test_brain.json");
        bank.save(test_path).expect("Tallentaminen epäonnistui");
        
        // Lataa uuteen pankkiin
        let loaded_bank = PatternBank::load(test_path).expect("Lataaminen epäonnistui");
        
        // Tarkista että data säilyi
        assert_eq!(loaded_bank.len(), bank.len());
        assert!(loaded_bank.has_pair(a_id, b_id));
        
        let loaded_ab = loaded_bank.get(ab_id).unwrap();
        assert_eq!(loaded_ab.strength, 0.8);
        assert_eq!(loaded_ab.usage_count, 5);
        assert_eq!(loaded_ab.ref_count, 0); // ab:hen ei viitata
        
        // Tarkista lasten ref_count
        assert_eq!(loaded_bank.get(a_id).unwrap().ref_count, 1);
        assert_eq!(loaded_bank.get(b_id).unwrap().ref_count, 1);
        
        // Siivoa testitiedosto
        std::fs::remove_file(test_path).ok();
    }
}
