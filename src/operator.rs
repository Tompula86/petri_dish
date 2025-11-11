use serde::{Deserialize, Serialize};

/// Operator (Operaattori): toimintauskomalli, joka muuntaa dataa ja 
/// jolla on kuvauskustannus ja hyödynnettävyyden mitta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Operator {
    /// RunLength(tavu, määrä): kuvaa toiston esim. "AAAAA" -> RunLength('A', 5)
    /// Koodaus: [OP_RLE, tavu, määrä]
    RunLength(u8, usize),
    /// GeneralizedRunLength(min_len): meta-malli, joka etsii minkä tahansa tavun
    /// vähintään min_len-mittaisia toistoja
    GeneralizedRunLength { min_len: usize },
    /// BackRef(distance, length): LZ-tyylinen viittaus aiempaan dataan
    /// Koodaus: [OP_LZ, dist_lo, dist_hi, length]
    BackRef(usize, usize),
    /// DeltaSequence(start, delta, len): kuvaa aritmeettista jonoa
    /// Koodaus: [OP_DELTA, len, start, delta]
    DeltaSequence { start: u8, delta: i8, len: usize },
    /// XorMask(key, base, len): kuvaa XOR-naamioitua vakiosekvenssiä
    /// Koodaus: [OP_XOR, len_lo, len_hi, key_len, base, key_bytes...]
    XorMask { key: Vec<u8>, base: u8, len: usize },
    /// Dictionary(word_id): viittaa sanakirjassa olevaan sanaan/lausekkeeseen
    /// Koodaus: [OP_DICT, word_id_lo, word_id_hi] = 3 tavua vs täysi sana
    Dictionary { word_id: u32 },
}

/// Operaattorikoodit binäärimuodossa
pub const OP_RLE: u8 = 0xFF; // RunLength-operaattorin tunniste
pub const OP_LZ: u8  = 0xFE; // BackRef-operaattorin tunniste
pub const OP_DELTA: u8 = 0xFD; // DeltaSequence-operaattorin tunniste
pub const OP_XOR: u8 = 0xFC; // XorMask-operaattorin tunniste
pub const OP_DICT: u8 = 0xFB; // Dictionary-operaattorin tunniste

impl Operator {
    /// Laske operaattorin koodauskustannus tavuina
    pub fn encoding_cost(&self) -> usize {
        match self {
            // OP_RLE + tavu + määrä = 3 tavua (yksinkertaistettu)
            Operator::RunLength(_, _) => 3,
            Operator::GeneralizedRunLength { .. } => 3,
            // OP_LZ + 2B distance + 1B length = 4 tavua
            Operator::BackRef(_, _) => 4,
            // OP_DELTA + len + start + delta = 4 tavua
            Operator::DeltaSequence { .. } => 4,
            // OP_XOR + 2B len + 1B key_len + 1B base + key_len tavua
            Operator::XorMask { key, .. } => 5 + key.len(),
            // OP_DICT + word_id (2 tavua) = 3 tavua
            Operator::Dictionary { .. } => 3,
        }
    }

    /// Kuinka monta tavua alkuperäistä dataa tämä operaattori korvaa
    pub fn replaced_bytes(&self) -> usize {
        match self {
            Operator::RunLength(_, count) => *count,
            Operator::GeneralizedRunLength { min_len } => *min_len,
            Operator::BackRef(_, len) => *len,
            Operator::DeltaSequence { len, .. } => *len,
            Operator::XorMask { len, .. } => *len,
            // Dictionary palauttaa 0 koska korvattavien tavujen määrä riippuu sanakirjasta
            // Tämä määritetään solver.rs:ssä kun haetaan varsinainen sana
            Operator::Dictionary { .. } => 0,
        }
    }
}
