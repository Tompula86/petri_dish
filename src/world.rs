use std::ops::Range;

/// World: rajattu muistialue (esim. 100 kB), jossa data elää ja johon transformaatioita sovelletaan.
pub struct World {
    pub data: Vec<u8>,
    pub memory_limit: usize,
}

/// Patch: kuvaa muutoksen, joka kohdistuu Worldin tiettyyn alueeseen
#[derive(Debug, Clone)]
pub struct Patch {
    pub range: Range<usize>,
    pub new_data: Vec<u8>,
}

impl World {
    pub fn new(memory_limit: usize) -> Self {
        World {
            data: Vec::with_capacity(memory_limit),
            memory_limit,
        }
    }

    pub fn load_data(&mut self, data: Vec<u8>) -> Result<(), &'static str> {
        if data.len() > self.memory_limit {
            return Err("Data exceeds memory limit");
        }
        self.data = data;
        Ok(())
    }

    /// Hae data annetulta alueelta (tarvitaan rollback-toimintoon)
    pub fn get_data_in_range(&self, range: Range<usize>) -> Vec<u8> {
        self.data[range].to_vec()
    }

    /// Sovella Patch: korvaa range-alue new_data:lla
    pub fn apply_patch(&mut self, patch: &Patch) {
        // Poista vanha alue ja korvaa uudella
        self.data.splice(patch.range.clone(), patch.new_data.iter().cloned());
    }

    /// Kumoa Patch: palauta alkuperäinen data
    pub fn rollback(&mut self, patch: &Patch, original_data: Vec<u8>) {
        // Laske nykyisen datan koko patch-alueella
        let current_len = patch.range.start + patch.new_data.len();
        let rollback_range = patch.range.start..current_len;
        
        self.data.splice(rollback_range, original_data.iter().cloned());
    }
}
