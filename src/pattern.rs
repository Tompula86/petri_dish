use crate::operator::Operator;
use serde::{Deserialize, Serialize, Deserializer, Serializer};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod systemtime_as_secs {
    use super::*;

    pub fn serialize<S>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
        serializer.serialize_u64(duration.as_secs())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + Duration::from_secs(secs))
    }
}

/// Pattern (Malli): esitys tavasta kuvata jokin datan rakenne lyhyemmin 
/// (esim. toisto, sanakirja, säännöllinen rakenne tai meta-sääntö).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    pub id: u32,
    pub operator: Operator,
    /// Kuinka monta kertaa tätä mallia on käytetty
    pub usage_count: u32,
    /// Kuinka paljon tavuja on säästetty tällä mallilla yhteensä
    pub total_bytes_saved: i32,
    /// Viimeisin käyttöaika (meta/unohtamista varten)
    #[serde(with = "systemtime_as_secs")]
    pub last_used: SystemTime,
    /// Viimeaikainen hyöty (exponentiaalinen keskiarvo)
    pub recent_gain: f64,
}

impl Pattern {
    pub fn new(id: u32, operator: Operator) -> Self {
        Pattern {
            id,
            operator,
            usage_count: 0,
            total_bytes_saved: 0,
            last_used: SystemTime::UNIX_EPOCH,
            recent_gain: 0.0,
        }
    }

    /// Päivitä tilastot onnistuneen sovelluksen jälkeen
    pub fn record_usage(&mut self, bytes_saved: i32) {
        const RECENT_GAIN_ALPHA: f64 = 0.25;
        self.usage_count += 1;
        self.total_bytes_saved += bytes_saved;
        self.last_used = SystemTime::now();
        let gain = bytes_saved as f64;
        if self.usage_count == 1 {
            self.recent_gain = gain;
        } else {
            self.recent_gain = (1.0 - RECENT_GAIN_ALPHA) * self.recent_gain + RECENT_GAIN_ALPHA * gain;
        }
    }
}
