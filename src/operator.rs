use serde::{Deserialize, Serialize};

/// Operator (Operaattori): Hierarkkinen "Lego-torni" arkkitehtuuri.
/// 
/// Ydinfilosofia: "Totuus on pysyvä yhteys kahden asian välillä."
/// 
/// Järjestelmä oppii kuten lapsi oppii kielen:
/// - Kirjaimista tavuihin
/// - Tavuista sanoihin
/// - Sanoista lauseisiin
/// - Lauseista ideoihin
#[derive(Debug, Clone, PartialEq, Hash, Eq, Serialize, Deserialize)]
pub enum Operator {
    /// Taso 0: Perusyksikkö (tavu/merkki), joka on annettu.
    /// Esim: 'a' = Literal(97)
    Literal(u8),
    
    /// Taso N: Yhdistää kaksi olemassa olevaa "totuutta" uudeksi,
    /// korkeamman tason totuudeksi.
    /// Esim: "t" + "a" = "ta" -> Combine(P_t, P_a)
    /// 
    /// PatternID:t viittaavat PatternBankissa oleviin malleihin.
    Combine(u32, u32),
}

impl Operator {
    /// Palauttaa hierarkiatason (complexity).
    /// - Literal = 0
    /// - Combine kahdesta Literalista = 1
    /// - Combine jossa toinen on Combine = max(left, right) + 1
    /// 
    /// Huom: Täydellinen kompleksisuuslaskenta vaatii pääsyn PatternBankiin,
    /// joten tämä on yksinkertaistettu versio.
    pub fn base_complexity(&self) -> u8 {
        match self {
            Operator::Literal(_) => 0,
            Operator::Combine(_, _) => 1, // Minimikompleksisuus yhdistelmälle
        }
    }
    
    /// Tarkistaa onko tämä Literal-operaattori
    pub fn is_literal(&self) -> bool {
        matches!(self, Operator::Literal(_))
    }
    
    /// Palauttaa Literal-tavun jos kyseessä on Literal
    pub fn as_literal(&self) -> Option<u8> {
        match self {
            Operator::Literal(b) => Some(*b),
            _ => None,
        }
    }
    
    /// Palauttaa Combine-parin jos kyseessä on Combine
    pub fn as_combine(&self) -> Option<(u32, u32)> {
        match self {
            Operator::Combine(left, right) => Some((*left, *right)),
            _ => None,
        }
    }
}
