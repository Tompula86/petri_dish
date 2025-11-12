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

    /// Kuinka paljon tilaa on vielä vapaana
    pub fn free_space(&self) -> usize {
        self.memory_limit.saturating_sub(self.data.len())
    }

    /// Laske maksimi-ikkunan koko annetulla murto-osalla muistista
    #[allow(dead_code)]
    pub fn limit_fraction(&self, fraction: f64) -> usize {
        let capped_fraction = fraction.clamp(0.0, 1.0);
        ((self.memory_limit as f64) * capped_fraction).round() as usize
    }

    /// Aseta ikkuna ankkuroituna datan loppuun
    #[allow(dead_code)]
    pub fn set_window_tail(&mut self, window_size: usize) {
        if self.data.is_empty() {
            self.window = 0..0;
            return;
        }

        let clamped = window_size.min(self.data.len());
        let end = self.data.len();
        let start = end.saturating_sub(clamped);
        self.window = start..end;
    }

    #[allow(dead_code)]
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
        self.data
            .splice(patch.range.clone(), patch.new_data.iter().cloned());
    }

    /// Kumoa Patch: palauta alkuperäinen data
    pub fn rollback(&mut self, patch: &Patch, original_data: Vec<u8>) {
        // Laske nykyisen datan koko patch-alueella
        let current_len = patch.range.start + patch.new_data.len();
        let rollback_range = patch.range.start..current_len;

        self.data
            .splice(rollback_range, original_data.iter().cloned());
    }

    /// Palauttaa viipaleen dataa nykyisen ikkunan kohdalta
    pub fn get_window_data(&self) -> &[u8] {
        // Varmista, että ikkuna on datan sisällä
        let start = self.window.start.min(self.data.len());
        let end = self.window.end.min(self.data.len());
        &self.data[start..end]
    }
}
