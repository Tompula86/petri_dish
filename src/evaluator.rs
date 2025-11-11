use crate::world::World;
use crate::operator::OP_RLE;

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
                // Operaattori: [OP_RLE, tavu, määrä]
                // C(model) = 3 (operaattorin koodaus)
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
