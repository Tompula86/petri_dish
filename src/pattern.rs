use crate::operator::Operator;

/// Pattern (Malli): esitys tavasta kuvata jokin datan rakenne lyhyemmin 
/// (esim. toisto, sanakirja, säännöllinen rakenne tai meta-sääntö).
#[derive(Debug, Clone)]
pub struct Pattern {
    pub id: u32,
    pub operator: Operator,
    /// Kuinka monta kertaa tätä mallia on käytetty
    pub usage_count: u32,
    /// Kuinka paljon tavuja on säästetty tällä mallilla yhteensä
    pub total_bytes_saved: i32,
}

impl Pattern {
    pub fn new(id: u32, operator: Operator) -> Self {
        Pattern {
            id,
            operator,
            usage_count: 0,
            total_bytes_saved: 0,
        }
    }

    /// Päivitä tilastot onnistuneen sovelluksen jälkeen
    pub fn record_usage(&mut self, bytes_saved: i32) {
        self.usage_count += 1;
        self.total_bytes_saved += bytes_saved;
    }
}
