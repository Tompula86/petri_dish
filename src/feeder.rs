// src/feeder.rs
use crate::builder::Builder;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::BufReader;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;

/// FeederState: Tämä tallennetaan levylle (kirjanmerkki)
#[derive(Serialize, Deserialize)]
pub struct FeederState {
    pub current_file_index: usize,
    pub current_file_pos: u64,
    pub total_fed: usize,
}

/// Feeder: "Striimaa" dataa kaikista .txt-tiedostoista annetussa kansiossa.
///
/// Uudessa arkkitehtuurissa Feeder syöttää dataa suoraan Builderiin,
/// joka tokenisoi sen.
///
/// Tukee nyt tilallisuutta: muistaa missä kohtaa dataa ollaan ja voi
/// jatkaa siitä mihin jäätiin (kirjanmerkki).
pub struct Feeder {
    pub feed_rate: usize,
    #[allow(dead_code)]
    base_feed_rate: usize,
    file_paths: Vec<PathBuf>,
    current_file_index: usize,
    current_file_pos: u64, // Missä tavussa mennään nykyisessä tiedostossa
    current_file: Option<BufReader<File>>,
    is_depleted: bool,
    /// Yhteensä syötetty tavumäärä
    pub total_fed: usize,
}

impl Feeder {
    /// Luo uuden Feederin, joka etsii kaikki .txt-tiedostot data_dir_path-kansiosta
    pub fn new(feed_rate: usize, data_dir_path: &str) -> io::Result<Self> {
        println!(
            "  📥 Feeder: Etsitään datatiedostoja kansiosta '{}'...",
            data_dir_path
        );

        let mut file_paths = Vec::new();

        // Rekursiivinen haku: etsii myös alikansioista
        Self::find_txt_files(data_dir_path, &mut file_paths)?;

        file_paths.sort();

        println!(
            "  📥 Feeder: Löydettiin {} .txt-tiedostoa.",
            file_paths.len()
        );
        for (i, path) in file_paths.iter().enumerate() {
            println!("     {}: {}", i + 1, path.display());
        }

        Ok(Feeder {
            feed_rate,
            base_feed_rate: feed_rate,
            file_paths,
            current_file_index: 0,
            current_file_pos: 0, // Alussa 0
            current_file: None,
            is_depleted: false,
            total_fed: 0,
        })
    }

    /// Tallenna Feederin tila (kirjanmerkki)
    pub fn save_state(&self, path: &str) -> io::Result<()> {
        let state = FeederState {
            current_file_index: self.current_file_index,
            current_file_pos: self.current_file_pos,
            total_fed: self.total_fed,
        };
        let json = serde_json::to_string_pretty(&state)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Lataa Feederin tila (kirjanmerkki)
    pub fn load_state(&mut self, path: &str) {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(state) = serde_json::from_str::<FeederState>(&content) {
                println!("  🔖 Feeder: Ladattiin kirjanmerkki.");
                println!(
                    "     Jatketaan tiedostosta indeksi {} kohdasta {}.",
                    state.current_file_index, state.current_file_pos
                );

                self.current_file_index = state.current_file_index;
                self.current_file_pos = state.current_file_pos;
                self.total_fed = state.total_fed;

                // Nollaa nykyinen tiedostokahva jotta open_next_file avaa sen oikein
                self.current_file = None;
            }
        }
    }

    /// Rekursiivinen .txt-tiedostojen etsintä
    fn find_txt_files(dir_path: &str, file_paths: &mut Vec<PathBuf>) -> io::Result<()> {
        for entry in fs::read_dir(dir_path)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                // Rekursiivisesti alikansioihin
                if let Some(path_str) = path.to_str() {
                    Self::find_txt_files(path_str, file_paths)?;
                }
            } else if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "txt" {
                        file_paths.push(path);
                    }
                }
            }
        }
        Ok(())
    }

    /// Apufunktio, joka avaa seuraavan tiedoston listalta JA kelaa oikeaan kohtaan
    fn open_next_file(&mut self) -> io::Result<()> {
        if let Some(path) = self.file_paths.get(self.current_file_index) {
            println!("  📥 Feeder: Avataan tiedosto '{}'...", path.display());

            let mut file = File::open(path)?;

            // Jos meillä on offset (pos > 0), hypätään sinne!
            if self.current_file_pos > 0 {
                println!("     ⏩ Kelataan kohtaan {}...", self.current_file_pos);
                file.seek(SeekFrom::Start(self.current_file_pos))?;
            }

            self.current_file = Some(BufReader::new(file));
            // HUOM: Älä kasvata indexiä tässä, se tehdään vasta kun tiedosto on loppu!
        } else {
            println!("  📥 Feeder: Kaikki datatiedostot käsitelty.");
            self.is_depleted = true;
            self.current_file = None;
        }
        Ok(())
    }

    /// Syötä seuraava pala dataa suoraan Builderiin (tokenisoi samalla)
    pub fn feed_to_builder(&mut self, builder: &mut Builder) -> Result<usize, String> {
        if self.is_depleted {
            return Ok(0);
        }

        if self.current_file.is_none() {
            self.open_next_file().map_err(|e| e.to_string())?;
            if self.is_depleted {
                return Ok(0);
            }
        }

        if let Some(ref mut file) = self.current_file {
            let mut buffer = vec![0u8; self.feed_rate];

            match file.read(&mut buffer) {
                Ok(0) => {
                    // Tiedosto loppui
                    println!(
                        "  📥 Feeder: Tiedosto '{}' luettu loppuun.",
                        self.file_paths[self.current_file_index].display()
                    );
                    self.current_file = None;
                    self.current_file_index += 1; // Siirry seuraavaan
                    self.current_file_pos = 0; // Nollaa positio seuraavaa varten

                    // Rekursiivinen kutsu jotta ei tule tyhjä sykli
                    self.feed_to_builder(builder)
                }
                Ok(bytes_read) => {
                    // Tokenisoi suoraan Builderiin
                    builder.tokenize(&buffer[..bytes_read]);
                    self.total_fed += bytes_read;
                    self.current_file_pos += bytes_read as u64; // Päivitä positio
                    Ok(bytes_read)
                }
                Err(e) => Err(e.to_string()),
            }
        } else {
            self.is_depleted = true;
            Ok(0)
        }
    }

    /// Tarkista, onko kaikki data syötetty
    pub fn is_depleted(&self) -> bool {
        self.is_depleted
    }

    /// Aseta syöttönopeus
    #[allow(dead_code)]
    pub fn set_feed_rate(&mut self, rate: usize) {
        self.feed_rate = rate.max(1);
    }

    /// Palauta perusnopeus
    #[allow(dead_code)]
    pub fn reset_feed_rate(&mut self) {
        self.feed_rate = self.base_feed_rate;
    }
}
