mod builder;
mod evaluator;
mod feeder;
mod operator;
mod pattern;

use builder::{Builder, PatternBank};
use evaluator::Evaluator;
use feeder::Feeder;

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;

/// Oletuspolku aivojen (PatternBank) tallennustiedostolle
const BRAIN_FILE_PATH: &str = "brain.json";

struct Config {
    /// Maksimi mallien määrä PatternBankissa (paitsi 256 literaalia)
    pattern_capacity: usize,
    /// Syöttönopeus tavuina per sykli
    feed_rate: usize,
    /// Parin esiintymiskynnys (montako kertaa pitää esiintyä)
    pair_threshold: u32,
    /// Maksimi syklien määrä
    max_cycles: usize,
    /// Polku aivojen tallennustiedostolle
    brain_path: String,
}

impl Config {
    const DEFAULT_PATTERN_CAPACITY: usize = 1000;
    const DEFAULT_FEED_RATE: usize = 500;
    const DEFAULT_PAIR_THRESHOLD: u32 = 2;
    const DEFAULT_MAX_CYCLES: usize = 200;

    fn load() -> Self {
        let pattern_capacity = env::var("PETRI_PATTERN_CAPACITY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(Self::DEFAULT_PATTERN_CAPACITY);

        let feed_rate = env::var("PETRI_FEED_RATE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(Self::DEFAULT_FEED_RATE);

        let pair_threshold = env::var("PETRI_PAIR_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(Self::DEFAULT_PAIR_THRESHOLD);

        let max_cycles = env::var("PETRI_MAX_CYCLES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(Self::DEFAULT_MAX_CYCLES);

        let brain_path = env::var("PETRI_BRAIN_PATH")
            .unwrap_or_else(|_| BRAIN_FILE_PATH.to_string());

        Config {
            pattern_capacity,
            feed_rate,
            pair_threshold,
            max_cycles,
            brain_path,
        }
    }
}

/// Lataa PatternBank tiedostosta tai luo uusi
fn load_or_create_brain(config: &Config) -> PatternBank {
    let path = Path::new(&config.brain_path);
    
    if path.exists() {
        println!("  🧠 Ladataan aivot tiedostosta '{}'...", config.brain_path);
        match PatternBank::load(path) {
            Ok(bank) => {
                println!("  ✅ Aivot ladattu! {} mallia muistissa.", bank.len());
                return bank;
            }
            Err(e) => {
                println!("  ⚠️  Aivojen lataus epäonnistui: {}", e);
                println!("     Aloitetaan tyhjästä...");
            }
        }
    } else {
        println!("  🧠 Aivotiedostoa '{}' ei löytynyt, aloitetaan tyhjästä.", config.brain_path);
    }
    
    PatternBank::new(config.pattern_capacity)
}

/// Tallenna PatternBank tiedostoon
fn save_brain(bank: &PatternBank, path: &str) {
    let path = Path::new(path);
    match bank.save(path) {
        Ok(()) => println!("  💾 Aivot tallennettu tiedostoon '{}'.", path.display()),
        Err(e) => println!("  ⚠️  Aivojen tallennus epäonnistui: {}", e),
    }
}

fn main() {
    println!("=== Petrimalja Älykkyyelle: HIERARKKINEN TIEDONRAKENNUSKONE ===\n");
    println!("Ydinfilosofia: \"Totuus on pysyvä yhteys kahden asian välillä.\"\n");
    println!("Petri Dish 2.0: \"Ikuinen Oppija\" - Pysyvä muisti käytössä.\n");

    let config = Config::load();

    // Lataa olemassa olevat aivot tai luo uudet
    let brain = load_or_create_brain(&config);
    
    // Luo Builder ladatulla PatternBankilla
    let mut builder = Builder::with_bank(brain);
    builder.pair_threshold = config.pair_threshold;

    // Luo Feeder
    let feeder_result = Feeder::new(config.feed_rate, "./data");
    
    let mut feeder = match feeder_result {
        Ok(f) => f,
        Err(e) => {
            println!("⚠️  Datakansio './data' ei löydy tai on tyhjä: {}", e);
            println!("    Luodaan esimerkkidata demonstraatiota varten...\n");
            
            // Luo esimerkkidata suoraan Builderiin
            let sample_text = b"funktio on joka funktio on joka funktio on joka \
                               tama on esimerkki tama on esimerkki tama on esimerkki \
                               alku alku alku loppu loppu loppu \
                               aabbaabbaabb ccddccddccdd";
            
            builder.tokenize(sample_text);
            
            println!("Aloitustilanne (esimerkkidata):");
            println!("  Syötetty: {} tavua", sample_text.len());
            println!("  Token-virta: {} tokenia", builder.stream_len());
            println!("  PatternBank: {} mallia (256 literaalia)", builder.bank.len());
            println!("\n--- Aloitetaan hierarkkinen oppiminen ---\n");
            
            // Aja oppimissyklit
            let evaluator = Evaluator::new();
            
            for _ in 0..config.max_cycles {
                let stats = builder.live();
                
                if stats.patterns_created > 0 || stats.patterns_collapsed > 0 {
                    stats.print();
                }
                
                // Lopeta jos virta ei enää tiivisty
                if stats.stream_before == stats.stream_after && stats.patterns_created == 0 {
                    break;
                }
            }
            
            println!("\n=== LOPPUTILANNE ===");
            evaluator.print_analysis(&builder);
            
            // Tulosta muutama esimerkki opituista malleista
            println!("\n  🧬 Opitut hierarkkiset mallit:");
            let mut patterns: Vec<_> = builder.bank.iter()
                .filter(|(_, p)| !p.is_literal() && p.strength >= 0.5)
                .collect();
            patterns.sort_by(|a, b| b.1.usage_count.cmp(&a.1.usage_count));
            
            for (id, pattern) in patterns.iter().take(10) {
                let decoded = builder.bank.decode(**id);
                let decoded_str = String::from_utf8_lossy(&decoded);
                println!(
                    "     P_{}: \"{}\" [taso {}, käyttö {}, vahvuus {:.2}, viittauksia {}]",
                    id, decoded_str, pattern.complexity, pattern.usage_count, pattern.strength, pattern.ref_count
                );
            }
            
            // Tallenna aivot
            save_brain(&builder.bank, &config.brain_path);
            
            println!("\n✅ Demonstraatio valmis!");
            return;
        }
    };

    // Luo Evaluator
    let evaluator = Evaluator::new();

    println!("\nAloitustilanne:");
    println!("  PatternBank kapasiteetti: {} mallia", builder.bank.capacity);
    println!("  Olemassa olevia malleja: {} (256 literaalia + {} combine)", builder.bank.len(), builder.bank.combine_count());
    println!("  Feeder nopeus: {} tavua/sykli", config.feed_rate);
    println!("  Parin kynnys: {} esiintymää", config.pair_threshold);
    println!("  Maksimi syklit: {}", config.max_cycles);
    println!("  Aivojen tallennuspolku: {}", config.brain_path);

    // Avaa CSV-tiedosto
    let mut csv_file = File::create("results.csv").expect("CSV-tiedoston luonti epäonnistui");
    writeln!(
        csv_file,
        "cycle,stream_len,original_len,patterns_count,compression_ratio,patterns_created,patterns_collapsed"
    )
    .expect("CSV-otsikkojen kirjoitus epäonnistui");

    println!("\n--- Aloitetaan hierarkkinen oppiminen ---\n");

    // Pääsilmukka
    let mut cycle = 0;
    let mut last_stream_len = 0;
    let mut stagnant_cycles = 0;

    while cycle < config.max_cycles {
        cycle += 1;

        // Syötä uutta dataa
        match feeder.feed_to_builder(&mut builder) {
            Ok(fed) => {
                if fed > 0 {
                    println!("  📥 Sykli {}: +{} tavua syötetty", cycle, fed);
                }
            }
            Err(e) => {
                println!("❌ Virhe: {}", e);
                break;
            }
        }

        // Aja oppimissykli
        let stats = builder.live();
        stats.print();

        // Kirjoita CSV
        writeln!(
            csv_file,
            "{},{},{},{},{:.4},{},{}",
            cycle,
            builder.stream_len(),
            builder.original_len(),
            builder.bank.combine_count(),
            evaluator.compression_ratio(&builder),
            stats.patterns_created,
            stats.patterns_collapsed
        )
        .expect("CSV-rivin kirjoitus epäonnistui");

        // Tarkista stagnaatio
        if builder.stream_len() == last_stream_len {
            stagnant_cycles += 1;
        } else {
            stagnant_cycles = 0;
        }
        last_stream_len = builder.stream_len();

        // Lopeta jos feeder on tyhjä ja stagnaatio jatkuu
        if feeder.is_depleted() && stagnant_cycles >= 5 {
            println!("\n  ✓ Oppiminen saturoitunut ({} sykliä ilman muutosta)", stagnant_cycles);
            break;
        }
    }

    // Loppuraportti
    println!("\n=== LOPPUTILANNE ===");
    
    if feeder.is_depleted() {
        println!("✅ Kaikki data käsitelty!");
    } else {
        println!("⚠️  Keskeytettiin syklien maksimirajalla ({}).", config.max_cycles);
    }

    evaluator.print_analysis(&builder);

    println!("\n  📊 Tilastot:");
    println!("     Syklit: {}", cycle);
    println!("     Syötetty: {} tavua", feeder.total_fed);
    println!("     Token-virta: {} tokenia", builder.stream_len());
    println!("     Combine-malleja: {}", builder.bank.combine_count());

    // Tulosta hierarkkiset mallit
    println!("\n  🧬 Opitut hierarkkiset mallit (TOP 20):");
    let mut patterns: Vec<_> = builder.bank.iter()
        .filter(|(_, p)| !p.is_literal())
        .collect();
    patterns.sort_by(|a, b| {
        // Lajittele: ensin tason mukaan (korkein ensin), sitten käytön mukaan
        let level_cmp = b.1.complexity.cmp(&a.1.complexity);
        if level_cmp == std::cmp::Ordering::Equal {
            b.1.usage_count.cmp(&a.1.usage_count)
        } else {
            level_cmp
        }
    });

    for (id, pattern) in patterns.iter().take(20) {
        let decoded = builder.bank.decode(**id);
        let decoded_str = String::from_utf8_lossy(&decoded);
        let preview = if decoded_str.len() > 30 {
            format!("{}...", &decoded_str[..30])
        } else {
            decoded_str.to_string()
        };
        println!(
            "     P_{}: \"{}\" [L{}, käyttö {}, str {:.2}, refs {}]",
            id, preview, pattern.complexity, pattern.usage_count, pattern.strength, pattern.ref_count
        );
    }

    // Tulosta hierarkiaesimerkki korkeimman tason mallista
    if let Some((id, _)) = patterns.first() {
        println!("\n  🌳 Hierarkiaesimerkki (P_{}):", id);
        builder.print_hierarchy(**id, 2);
    }

    // Tallenna aivot
    save_brain(&builder.bank, &config.brain_path);

    println!("\n=== HIERARKKINEN TIEDONRAKENNUSKONE VALMIS ===");
    println!("\n📊 Analyysi:");
    println!("  • CSV tallennettu: results.csv");
    println!("  • Aivot tallennettu: {}", config.brain_path);
    println!("  • Järjestelmä oppi kielen rakenteita hierarkkisesti");
    println!("  • Kirjaimista → tavuihin → sanoihin → lauseisiin");
    println!("\n✅ \"Totuus on pysyvä yhteys kahden asian välillä.\"");
}

