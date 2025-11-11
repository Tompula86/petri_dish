// src/feeder.rs
use crate::world::World;
use std::fs::{self, File};
use std::io::{self, Read};
use std::io::BufReader;
use std::path::PathBuf;

/// Feeder: "Striimaa" dataa kaikista .txt-tiedostoista annetussa kansiossa.
pub struct Feeder {
    pub feed_rate: usize,
    base_feed_rate: usize,
    file_paths: Vec<PathBuf>, // Lista kaikista .txt-tiedostoista
    current_file_index: usize, // Monesko tiedosto menossa
    current_file: Option<BufReader<File>>, // Kahva auki olevaan tiedostoon
    is_depleted: bool, // Onko kaikki tiedostot luettu?
}

impl Feeder {
    /// Luo uuden Feederin, joka etsii kaikki .txt-tiedostot data_dir_path-kansiosta
    pub fn new(feed_rate: usize, data_dir_path: &str) -> io::Result<Self> {
        println!("  📥 Feeder: Etsitään datatiedostoja kansiosta '{}'...", data_dir_path);

        let mut file_paths = Vec::new();
        // Lue kansion sisältö
        for entry in fs::read_dir(data_dir_path)? {
            let entry = entry?;
            let path = entry.path();
            // Hyväksy vain tiedostot, joiden pääte on .txt
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "txt" {
                        file_paths.push(path);
                    }
                }
            }
        }

        file_paths.sort(); // Varmistetaan johdonmukainen lukujärjestys

        println!("  📥 Feeder: Löydettiin {} .txt-tiedostoa.", file_paths.len());
        for (i, path) in file_paths.iter().enumerate() {
            println!("     {}: {}", i + 1, path.display());
        }

        Ok(Feeder {
            feed_rate,
            base_feed_rate: feed_rate,
            file_paths,
            current_file_index: 0,
            current_file: None, // Avataan tiedosto vasta kun 'feed' kutsutaan
            is_depleted: false,
        })
    }

    /// Apufunktio, joka avaa seuraavan tiedoston listalta
    fn open_next_file(&mut self) -> io::Result<()> {
        if let Some(path) = self.file_paths.get(self.current_file_index) {
            println!("  📥 Feeder: Avataan tiedosto '{}'...", path.display());
            let file = File::open(path)?;
            self.current_file = Some(BufReader::new(file));
            self.current_file_index += 1;
        } else {
            // Ei enää tiedostoja. Kaikki data on syötetty.
            println!("  📥 Feeder: Kaikki datatiedostot käsitelty.");
            self.is_depleted = true;
            self.current_file = None;
        }
        Ok(())
    }

    /// Syötä seuraava pala dataa Worldiin
    pub fn feed(&mut self, world: &mut World) -> Result<usize, String> {
        if self.is_depleted {
            return Ok(0);
        }

        // Jos tiedosto ei ole auki (tai edellinen loppui), yritä avata seuraava
        if self.current_file.is_none() {
            self.open_next_file().map_err(|e| e.to_string())?;
            // Jos se on edelleen 'None', kaikki tiedostot on luettu
            if self.is_depleted {
                return Ok(0);
            }
        }

        // Nyt meillä pitäisi olla tiedosto auki. Luetaan siitä.
        if let Some(ref mut file) = self.current_file {
            // Sovita syötteen koko nykyiseen vapaaseen tilaan
            let free_space = world.free_space();
            if free_space == 0 {
                return Err("OVERFLOW: World täynnä! Solver ei vapauta tilaa tarpeeksi nopeasti.".to_string());
            }

            let adaptive_cap = (free_space / 2).max(1);
            self.feed_rate = self.base_feed_rate.min(free_space).min(adaptive_cap);
            if self.feed_rate != self.base_feed_rate {
                println!(
                    "  📥 Feeder: hidastetaan syöttöä {} tavuun (vapaa tila: {})",
                    self.feed_rate,
                    free_space
                );
            }

            // Luodaan puskuri *vain* tarvittavalle määrälle
            let mut buffer = vec![0u8; self.feed_rate];
            
            match file.read(&mut buffer) {
                Ok(0) => {
                    // 0 tavua luettu = tiedosto loppui.
                    println!("  📥 Feeder: Tiedosto '{}' luettu loppuun.", self.file_paths[self.current_file_index - 1].display());
                    self.current_file = None; // Sulje tiedosto
                    // Kutsu feed() uudestaan *tämän saman syklin aikana*
                    // avataksesi seuraavan tiedoston heti.
                    self.feed(world)
                }
                Ok(bytes_read) => {
                    // Dataa luettu. Tarkista World-rajoitus.
                    if world.data.len() + bytes_read > world.memory_limit {
                        return Err("OVERFLOW: World täynnä! Feeder nopeampi kuin Solver.".to_string());
                    }
                    
                    // HUOM: buffer on 'feed_rate' kokoinen, mutta luimme vain 'bytes_read'
                    world.data.extend_from_slice(&buffer[..bytes_read]);
                    Ok(bytes_read)
                }
                Err(e) => {
                    // Jokin meni pieleen tiedostoa lukiessa
                    Err(e.to_string())
                }
            }
        } else {
            // Tänne ei pitäisi päätyä, mutta varmuuden vuoksi
            self.is_depleted = true;
            Ok(0)
        }
    }

    /// Tarkista, onko kaikki data syötetty
    pub fn is_depleted(&self) -> bool {
        self.is_depleted
    }
}
