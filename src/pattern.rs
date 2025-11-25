use crate::operator::Operator;
use serde::{Deserialize, Serialize};

/// Pattern (Malli): Elävä hypoteesi hierarkkisessa oppimissysteemissä.
/// 
/// Malli ei ole staattinen sääntö. Se on elävä hypoteesi, joka:
/// - Vahvistuu käytöstä
/// - Heikkenee virheistä
/// - Voi "kuolla" (unohtua) jos se on liian heikko
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    /// Uniikki tunniste PatternBankissa
    pub id: u32,
    
    /// Operaattori: Literal(u8) tai Combine(u32, u32)
    pub op: Operator,
    
    /// "Totuusarvo": 0.0 - 1.0
    /// - Vahvistuu kun ennustus osuu oikein
    /// - Heikkenee kun ennustus epäonnistuu
    pub strength: f64,
    
    /// Aikaleima (sykli) unohtamista varten.
    /// Kun malli ei ole ollut käytössä pitkään aikaan,
    /// se voidaan "unohtaa" (evict).
    pub last_used: u64,
    
    /// Hierarkian taso (complexity):
    /// - Literal = 0
    /// - Combine(Lit, Lit) = 1
    /// - Combine(Combine, Lit) = 2
    /// - jne.
    pub complexity: u8,
    
    /// Kuinka monta kertaa tätä mallia on käytetty onnistuneesti
    pub usage_count: u32,
}

impl Pattern {
    /// Luo uusi Literal-malli (taso 0)
    pub fn new_literal(id: u32, byte: u8) -> Self {
        Pattern {
            id,
            op: Operator::Literal(byte),
            strength: 1.0, // Literaalit ovat aina "tosia"
            last_used: 0,
            complexity: 0,
            usage_count: 0,
        }
    }
    
    /// Luo uusi Combine-malli (taso N)
    /// 
    /// Kompleksisuus lasketaan: max(left_complexity, right_complexity) + 1
    pub fn new_combine(id: u32, left_id: u32, right_id: u32, left_complexity: u8, right_complexity: u8, cycle: u64) -> Self {
        let complexity = left_complexity.max(right_complexity).saturating_add(1);
        Pattern {
            id,
            op: Operator::Combine(left_id, right_id),
            strength: 0.5, // Uudet yhdistelmät alkavat keskitasolta
            last_used: cycle,
            complexity,
            usage_count: 0,
        }
    }
    
    /// Vahvista mallin "totuusarvoa" kun ennustus osuu oikein
    pub fn strengthen(&mut self, amount: f64, cycle: u64) {
        self.strength = (self.strength + amount).min(1.0);
        self.last_used = cycle;
        self.usage_count += 1;
    }
    
    /// Heikennä mallin "totuusarvoa" kun ennustus epäonnistuu
    pub fn weaken(&mut self, amount: f64) {
        self.strength = (self.strength - amount).max(0.0);
    }
    
    /// Tarkista onko malli "kuollut" (liian heikko)
    pub fn is_dead(&self, threshold: f64) -> bool {
        self.strength < threshold
    }
    
    /// Tarkista onko tämä Literal-malli
    pub fn is_literal(&self) -> bool {
        self.op.is_literal()
    }
    
    /// Palauttaa Combine-parin jos kyseessä on Combine
    pub fn as_combine(&self) -> Option<(u32, u32)> {
        self.op.as_combine()
    }
}
