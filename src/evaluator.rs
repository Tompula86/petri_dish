use crate::builder::Builder;

/// Evaluator (Arvioija): Mittaa hierarkkisen oppimisen tehokkuutta.
/// 
/// Uudessa arkkitehtuurissa kustannus lasketaan:
/// - C(tokens): Token-virran pituus (kuinka monta symbolia)
/// - C(patterns): Mallien muistikustannus
/// - Tiivistyssuhde: alkuperäinen tavumäärä / token-määrä
pub struct Evaluator {}

impl Evaluator {
    pub fn new() -> Self {
        Evaluator {}
    }

    /// Laske kokonaiskustannus Builder-tilasta
    /// 
    /// Kustannus = token-virran pituus + mallien määrä / 10
    /// (mallien kustannus on pienempi koska ne ovat uudelleenkäytettäviä)
    #[allow(dead_code)]
    pub fn calculate_cost(&self, builder: &Builder) -> usize {
        let token_cost = builder.stream_len();
        let pattern_cost = builder.bank.combine_count() / 10;
        token_cost + pattern_cost
    }
    
    /// Laske tiivistyssuhde
    pub fn compression_ratio(&self, builder: &Builder) -> f64 {
        let original = builder.original_len();
        let compressed = builder.stream_len();
        
        if original == 0 {
            return 0.0;
        }
        
        1.0 - (compressed as f64 / original as f64)
    }
    
    /// Laske "bittikustannus" - teoreettinen minimikoodaus
    /// 
    /// Jokaiselle tokenille: log2(mallien_määrä) bittiä
    pub fn bit_cost(&self, builder: &Builder) -> f64 {
        let pattern_count = builder.bank.len();
        if pattern_count <= 1 {
            return 0.0;
        }
        
        let bits_per_token = (pattern_count as f64).log2();
        bits_per_token * builder.stream_len() as f64
    }
    
    /// Tulosta kustannusanalyysi
    pub fn print_analysis(&self, builder: &Builder) {
        let original_bytes = builder.original_len();
        let tokens = builder.stream_len();
        let patterns = builder.bank.combine_count();
        let ratio = self.compression_ratio(builder);
        let bits = self.bit_cost(builder);
        
        println!("  📊 Kustannusanalyysi:");
        println!("     Alkuperäinen: {} tavua", original_bytes);
        println!("     Token-virta: {} tokenia", tokens);
        println!("     Combine-malleja: {}", patterns);
        println!("     Tiivistyssuhde: {:.1}%", ratio * 100.0);
        println!("     Bittikustannus: {:.1} bittiä ({:.1} tavua)", bits, bits / 8.0);
    }
}
