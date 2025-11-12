use crate::operator::{OP_DELTA, OP_DICT, OP_LZ, OP_RLE, OP_XOR};
use crate::world::World;

/// Evaluator (Arvioija): mittaa kokonaiskustannuksen ja hyväksyy vain muutokset,
/// jotka parantavat nettoa.
pub struct Evaluator {}

impl Evaluator {
    pub fn new() -> Self {
        Evaluator {}
    }

    /// Kustannusfunktio: C_total = C(models) + C(residual)
    /// - C(models): operaattoreiden koodauskustannus
    /// - C(residual): "raaka" datan kustannus (tavut jotka eivät ole operaattoreita)
    pub fn calculate_total_cost(&self, world: &World) -> usize {
        let (c_models, c_residual) = self.calculate_cost_breakdown(world);
        c_models + c_residual
    }

    /// Palauttaa (C(models), C(residual)) erikseen
    pub fn calculate_cost_breakdown(&self, world: &World) -> (usize, usize) {
        let mut c_models = 0;
        let mut c_residual = 0;
        let data = &world.data;
        let mut i = 0;

        while i < data.len() {
            if data[i] == OP_RLE && i + 2 < data.len() {
                c_models += 3; // [OP_RLE, byte, count]
                i += 3;
            } else if data[i] == OP_LZ && i + 3 < data.len() {
                // [OP_LZ, dist_lo, dist_hi, len]
                c_models += 4;
                i += 4;
            } else if data[i] == OP_DELTA && i + 3 < data.len() {
                // [OP_DELTA, len, start, delta]
                c_models += 4;
                i += 4;
            } else if data[i] == OP_XOR && i + 4 < data.len() {
                let key_len = data[i + 3] as usize;
                let op_len = 5 + key_len; // [OP_XOR, len_lo, len_hi, key_len, base, key_bytes...]
                if i + op_len <= data.len() {
                    c_models += op_len;
                    i += op_len;
                } else {
                    // Virheellinen op-koodi tulkitaan residuaaliksi
                    c_residual += 1;
                    i += 1;
                }
            } else if data[i] == OP_DICT && i + 2 < data.len() {
                // [OP_DICT, word_id_lo, word_id_hi]
                c_models += 3;
                i += 3;
            } else {
                // Raaka data: kustannus = 1 tavu
                c_residual += 1;
                i += 1;
            }
        }

        (c_models, c_residual)
    }

    /// Laskee kuinka monta tavua hyödetään muutoksesta
    /// Positiivinen arvo = säästö, negatiivinen = tappio
    pub fn calculate_gain(&self, cost_before: usize, cost_after: usize) -> i32 {
        cost_before as i32 - cost_after as i32
    }
}
