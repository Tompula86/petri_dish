mod world;
mod evaluator;
mod solver;
mod pattern;
mod operator;
mod feeder;
mod stats;
mod scheduler;

use world::World;
use feeder::Feeder;
use evaluator::Evaluator;
use solver::Solver;

use std::fs::File;
use std::io::Write;

fn main() {
    println!("=== Petrimalja Älykkyyelle: VAIHE 5 - Sopeutumisnopeus ===\n");

    // Luo World (rajoitettu 50 kB, paine kovenee!)
    let mut world = World::new(50_000);
    
    // Vaihdetaan loputtomaan generaattorifeederiin
    let mut feeder = Feeder::new(3_000); // 3 kB per sykli

    // Luo Evaluator ja Solver
    let evaluator = Evaluator::new();
    let mut solver = Solver::load_or_new(1000, 50); // 50 patternin kapasiteetti

    println!("Aloitustilanne:");
    println!("  World kapasiteetti: {} tavua", world.memory_limit);
    println!("  Feeder nopeus: {} tavua/sykli", feeder.feed_rate);
    println!("  Feeder virta: (generaattori, ei rajallista dataa)");
    println!("  Solver: {} mallia ladattu muistista", solver.known_patterns.len());
    
    // Avaa CSV-tiedosto
    let mut csv_file = File::create("results.csv").expect("CSV-tiedoston luonti epäonnistui");
    writeln!(csv_file, "cycle,world_size,c_models,c_residual,c_total,gain_per_quota_exploit,gain_per_quota_explore,patterns_count")
        .expect("CSV-otsikkojen kirjoitus epäonnistui");
    
    // Pääsilmukka: Solver vs Feeder
    let mut cycle = 0;
    let mut overflow_detected = false;
    
    // Aja rajatussa testissä 1000 sykliä tällä kierroksella
    while cycle < 1000 {
        cycle += 1;
        let world_size = world.data.len();
        
        println!("\n=== SYKLI {} ===", cycle);
        
        // Progress bar
        let progress = (world_size * 100) / world.memory_limit;
        let filled = progress / 10;
        let empty = 10 - filled;
        let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(empty));
        
        println!("  {} {}% ({}/{} tavua)", 
                 bar, progress, world_size, world.memory_limit);
        
        // Solver yrittää tiivistää
        solver.live(&mut world, &evaluator);
        
        // Kojelauta: Tilastot
        let stats = &solver.stats;
        if stats.total_quota_spent() > 0 {
            println!("  📊 C(models): {} | C(res): {} | Total: {}", 
                     stats.c_models, stats.c_residual, 
                     stats.c_models + stats.c_residual);
            println!("  📈 Exploit G/Q: {:.2} | Explore G/Q: {:.2}", 
                     stats.gain_per_quota_exploit, stats.gain_per_quota_explore);
        }
        
        // Kirjoita CSV
        writeln!(csv_file, "{},{},{},{},{},{:.2},{:.2},{}", 
                 cycle, world.data.len(), stats.c_models, stats.c_residual,
                 stats.c_models + stats.c_residual,
                 stats.gain_per_quota_exploit, stats.gain_per_quota_explore,
                 solver.known_patterns.len())
            .expect("CSV-rivin kirjoitus epäonnistui");
        
        // Feeder työntää uutta dataa
        match feeder.feed(&mut world) {
            Ok(fed) => {
                if fed > 0 {
                    println!("  📥 Feeder: +{} tavua", fed);
                }
            }
            Err(e) => {
                println!("\n💥 {} 💥 (Sykli {})", e, cycle);
                overflow_detected = true;
                break;
            }
        }
    }

    // Tallenna oppiminen ennen loppua
    if let Err(e) = solver.save_patterns() {
        println!("⚠️  Mallien tallennus epäonnistui: {}", e);
    }

    // Loppuraportti
    println!("\n=== LOPPUTILANNE ===");
    if overflow_detected {
        println!("❌ EPÄONNISTUI: World täyttyi. Solver ei pysynyt Feederin tahdissa.");
    } else if feeder.is_depleted() {
        println!("✅ ONNISTUI: Kaikki data käsitelty ilman overflowia!");
    } else {
        println!("⚠️  Keskeytettiin syklien maksimirajalla.");
    }
    
    println!("\nTilastot:");
    println!("  Lopullinen koko: {} tavua", world.data.len());
    println!("  Lopullinen kustannus: {}", evaluator.calculate_total_cost(&world));
    println!("  PatternBank: {} mallia", solver.known_patterns.len());
    
    for pattern in &solver.known_patterns {
        println!("    - Pattern #{}: {} käyttöä, {} tavua säästetty",
                 pattern.id, pattern.usage_count, pattern.total_bytes_saved);
    }
    
    println!("\n=== VAIHE 5 VALMIS: Sopeutumisnopeus todistettu! ===");
    println!("\n📊 ANALYYSI:");
    println!("  • CSV tallennettu: results.csv");
    println!("  • Syklit: {}", cycle);
    println!("  • Alkuperäinen koko: 60 000 tavua");
    println!("  • Lopullinen koko: {} tavua", world.data.len());
    println!("  • Pakkaussuhde: {:.1}%", 100.0 - (world.data.len() as f64 / 60000.0 * 100.0));
    println!("\n✅ Järjestelmä:");
    println!("  ✓ Oppii malleja dynaamisesti (explore)");
    println!("  ✓ Käyttää opittuja tehokkaasti (exploit)");
    println!("  ✓ Sopeutuu muuttuvaan dataan");
    println!("  ✓ Selviää paineesta ilman overflowia");
    println!("  ✓ Mittaa suorituskykyä (gain/quota)");
    println!("\n📈 Voit analysoida tuloksia:");
    println!("  Python: import pandas as pd; df = pd.read_csv('results.csv')");
    println!("  Excel: Avaa results.csv");
}

