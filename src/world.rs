use std::ops::Range;

/// World: rajattu muistialue (esim. 100 kB), jossa data elää ja johon transformaatioita sovelletaan.
pub struct World {
    pub data: Vec<u8>,
    pub memory_limit: usize,
    /// FocusWindow: Solver näkee vain tämän ikkunan kerrallaan
    pub window: Range<usize>,
}

/// Patch: kuvaa muutoksen, joka kohdistuu Worldin tiettyyn alueeseen
#[derive(Debug, Clone)]
pub struct Patch {
    pub range: Range<usize>,
    pub new_data: Vec<u8>,
}

impl Patch {
    /// Luo kopio patchista, jossa range on siirretty offsetilla
    /// (muuntaa paikallisen ikkunan rangesta globaaliksi World-rangeksi)
    pub fn clone_with_offset(&self, offset: usize) -> Self {
        Patch {
            range: (self.range.start + offset)..(self.range.end + offset),
            new_data: self.new_data.clone(),
        }
    }
}

impl World {
    pub fn new(memory_limit: usize) -> Self {
        World {
            data: Vec::with_capacity(memory_limit),
            memory_limit,
            window: 0..0, // Ikkuna alustetaan tyhjäksi
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

    /// Palauttaa viipaleen dataa nykyisen ikkunan kohdalta
    pub fn get_window_data(&self) -> &[u8] {
        // Varmista, että ikkuna on datan sisällä
        let start = self.window.start.min(self.data.len());
        let end = self.window.end.min(self.data.len());
        &self.data[start..end]
    }

    /// Siirrä ikkunaa ja palauta siirron quota-kustannus
    /// (Yksinkertainen malli: 1 quota + 1 per 1000 siirrettyä tavua)
    pub fn shift_window(&mut self, new_start: usize, window_size: usize) -> u32 {
        let new_end = new_start + window_size;
        let distance = (new_start as i32 - self.window.start as i32).abs() as u32;
        self.window = new_start..new_end;

        let quota_cost = 1 + (distance / 1000);
        quota_cost
    }
}
