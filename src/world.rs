use std::ops::Range;

/// World: rajattu muistialue, jossa data elää.
/// 
/// Uudessa arkkitehtuurissa World säilyttää raaka-datan,
/// mutta Builder operoi token-virralla.
pub struct World {
    /// Raaka data tavuina
    pub data: Vec<u8>,
    
    /// Muistiraja
    pub memory_limit: usize,
    
    /// FocusWindow: Builder näkee vain tämän ikkunan kerrallaan
    pub window: Range<usize>,
}

impl World {
    pub fn new(memory_limit: usize) -> Self {
        World {
            data: Vec::with_capacity(memory_limit),
            memory_limit,
            window: 0..0,
        }
    }

    /// Kuinka paljon tilaa on vielä vapaana
    pub fn free_space(&self) -> usize {
        self.memory_limit.saturating_sub(self.data.len())
    }

    /// Lisää dataa Worldiin
    pub fn append(&mut self, data: &[u8]) -> Result<usize, &'static str> {
        let available = self.free_space();
        if available == 0 {
            return Err("World is full");
        }
        
        let to_add = data.len().min(available);
        self.data.extend_from_slice(&data[..to_add]);
        Ok(to_add)
    }

    /// Lataa koko data
    #[allow(dead_code)]
    pub fn load_data(&mut self, data: Vec<u8>) -> Result<(), &'static str> {
        if data.len() > self.memory_limit {
            return Err("Data exceeds memory limit");
        }
        self.data = data;
        Ok(())
    }

    /// Palauttaa viipaleen dataa nykyisen ikkunan kohdalta
    pub fn get_window_data(&self) -> &[u8] {
        let start = self.window.start.min(self.data.len());
        let end = self.window.end.min(self.data.len());
        &self.data[start..end]
    }
    
    /// Datan pituus
    pub fn len(&self) -> usize {
        self.data.len()
    }
    
    /// Onko tyhjä
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
