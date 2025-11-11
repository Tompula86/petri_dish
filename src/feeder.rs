use crate::world::World;

/// Feeder: Virtasyöttäjä joka työntää uutta dataa Worldiin konfiguroitavalla nopeudella.
/// Luo dynaamisen paineen: jos Solver ei pysy tiivistämään, muisti täyttyy.
pub struct Feeder {
    /// Datavir ta josta syötetään
    data_stream: Vec<u8>,
    /// Nykyinen positio virrassa
    position: usize,
    /// Kuinka monta tavua syötetään per feed()-kutsu
    pub feed_rate: usize,
}

impl Feeder {
    pub fn new(data_stream: Vec<u8>, feed_rate: usize) -> Self {
        Feeder {
            data_stream,
            position: 0,
            feed_rate,
        }
    }

    /// Syötä uutta dataa Worldiin
    /// Palauttaa: Ok(syötetty määrä) tai Err jos World täyttyi
    pub fn feed(&mut self, world: &mut World) -> Result<usize, &'static str> {
        // Laske kuinka paljon voidaan syöttää
        let remaining_in_stream = self.data_stream.len() - self.position;
        let to_feed = self.feed_rate.min(remaining_in_stream);

        if to_feed == 0 {
            return Ok(0); // Virta loppui
        }

        // Tarkista mahtuuko Worldiin
        if world.data.len() + to_feed > world.memory_limit {
            return Err("OVERFLOW: World täynnä! Feeder nopeampi kuin Solver.");
        }

        // Syötä data
        let end_pos = self.position + to_feed;
        world.data.extend_from_slice(&self.data_stream[self.position..end_pos]);
        self.position = end_pos;

        Ok(to_feed)
    }

    /// Kuinka paljon dataa on vielä jäljellä virrassa
    pub fn remaining(&self) -> usize {
        self.data_stream.len() - self.position
    }

    /// Onko virta lopussa
    pub fn is_depleted(&self) -> bool {
        self.position >= self.data_stream.len()
    }
}
