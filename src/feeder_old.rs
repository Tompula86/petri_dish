use crate::world::World;
use rand::Rng;

/// Feeder: generaattori, joka tuottaa datavirtaa loputtomasti (testiympäristöä varten)
pub struct Feeder {
    pub feed_rate: usize,
    cycle: u64,
}

impl Feeder {
    pub fn new(feed_rate: usize) -> Self {
        Feeder { feed_rate, cycle: 0 }
    }

    /// Syötä uutta dataa Worldiin
    pub fn feed(&mut self, world: &mut World) -> Result<usize, &'static str> {
        self.cycle += 1;
        let new_data = self.generate_data(self.feed_rate, self.cycle);

        if world.data.len() + new_data.len() > world.memory_limit {
            return Err("OVERFLOW: World täynnä! Feeder nopeampi kuin Solver.");
        }

        world.data.extend_from_slice(&new_data);
        Ok(new_data.len())
    }

    /// Proseduraalinen datan generointi
    fn generate_data(&self, amount: usize, cycle: u64) -> Vec<u8> {
        let mut data = Vec::with_capacity(amount);
        let mut rng = rand::thread_rng();

        match (cycle / 100) % 3 {
            0 => { // Tyyppi A: Toistoja
                for i in 0..amount { data.push((i % 10) as u8); }
            }
            1 => { // Tyyppi B: Korkea entropia (kohinaa)
                for _ in 0..amount { data.push(rng.gen_range(0..=255) as u8); }
            }
            _ => { // Tyyppi C: Harvat kuviot
                for _ in 0..amount { data.push(b'X'); }
                if data.len() > 10 { data[10] = b'Y'; }
            }
        }

        data
    }

    pub fn is_depleted(&self) -> bool { false }
}
