/// Operator (Operaattori): toimintauskomalli, joka muuntaa dataa ja 
/// jolla on kuvauskustannus ja hyödynnettävyyden mitta.
#[derive(Debug, Clone, PartialEq)]
pub enum Operator {
    /// RunLength(tavu, määrä): kuvaa toiston esim. "AAAAA" -> RunLength('A', 5)
    /// Koodaus: [OP_RLE, tavu, määrä]
    RunLength(u8, usize),
}

/// Operaattorikoodit binäärimuodossa
pub const OP_RLE: u8 = 0xFF; // RunLength-operaattorin tunniste

impl Operator {
    /// Laske operaattorin koodauskustannus tavuina
    pub fn encoding_cost(&self) -> usize {
        match self {
            // OP_RLE + tavu + määrä = 3 tavua (yksinkertaistettu)
            Operator::RunLength(_, _) => 3,
        }
    }

    /// Kuinka monta tavua alkuperäistä dataa tämä operaattori korvaa
    pub fn replaced_bytes(&self) -> usize {
        match self {
            Operator::RunLength(_, count) => *count,
        }
    }
}
