# Petri Dish - Suorituskykyparannukset

## Yhteenveto tehdyistä muutoksista

Tämä dokumentti listaa kaikki tehdyt optimoinnit, jotka parantavat Petri Dish -järjestelmän oppimis- ja pakkaustehoa.

### 1. MIN_ACCEPT_GAIN - Alennettu hyväksymiskynnys (KRIITTINEN)

**Tiedosto:** `src/solver.rs`  
**Muutos:** `const MIN_ACCEPT_GAIN: i32 = 25;` → `const MIN_ACCEPT_GAIN: i32 = 5;`

**Syy:** Alkuperäinen kynnysarvo 25 tavua oli liian korkea. Syklin 21 jälkeen kaikki uudet löydökset (lyhyet toistot, delta-sekvenssit) tuottivat alle 25 tavun hyödyn ja hylättiin → explore-funktio lakkasi oppimasta.

**Vaikutus:** Järjestelmä hyväksyy nyt myös pienet 5-25 tavun parannukset, mikä mahdollistaa jatkuvan oppimisen ja hienosäädön.

---

### 2. EXPLOIT_BEAM_WIDTH - Laajennettu mallien käyttö (KRIITTINEN)

**Tiedosto:** `src/solver.rs`  
**Muutos:** `const EXPLOIT_BEAM_WIDTH: usize = 100;` → `const EXPLOIT_BEAM_WIDTH: usize = 200;`

**Syy:** PatternBank tallentaa 150 mallia, mutta exploit() käsitteli vain 100 parasta mallia recent_gain-arvon perusteella. Tämä tarkoitti, että 50 mallia (mukaan lukien löydettyjä sanoja) jätettiin käyttämättä joka syklillä.

**Vaikutus:** Kaikki 150 mallia pankissa saavat nyt mahdollisuuden tulla käytetyiksi. Tämä erityisesti hyödyntää sanakirjasta löydettyjä sanoja.

---

### 3. Syklipohjainen sanakirjan päivitys - Vähennetty overhead

**Tiedosto:** `src/solver.rs`  
**Muutokset:**
- Lisätty `cycle_count: u32` Solver-structiin
- Muutettu `live()` funktiossa: sanakirja päivittyy nyt vain joka 10. syklillä

**Koodi:**
```rust
// Päivitä cycle-laskuri
self.cycle_count += 1;

// Päivitä sanakirja vain joka 10. syklillä
if self.cycle_count % 10 == 0 {
    self.refresh_dictionary(world);
}
```

**Syy:** `refresh_dictionary()` skannaa koko maailman datan joka syklillä, mikä on kallista ja täyttää PatternBankin liian aggressiivisesti sanaehdokkailla, syrjäyttäen muut hyödylliset mallit (RunLength, Delta).

**Vaikutus:** Explore-funktio saa enemmän aikaa löytää ja tallentaa muita mallityyppejä sanojen väleissä. Vähentää myös laskentakustannuksia.

---

### 4. Parannettu Explore - Lyhyemmät mallit

**Tiedosto:** `src/solver.rs`  
**Muutokset explore()-funktiossa:**

**4a. Lyhyemmät n-grammit (3-6 tavua)**
```rust
// Lisätty:
if let Some(p) = self.find_ngram_reference(slice, 3, 6) {
    consider(p);
}
```

**4b. Lyhyemmät backref-viittaukset**
```rust
// Muutettu: min_len 4 → 3
if let Some(p) = self.find_backref(world, 3, 16384) {
    consider(p);
}
```

**4c. Lyhyemmät delta-sekvenssit**
```rust
// Muutettu: min_len 6 → 5
if let Some(p) = self.find_delta_sequence(slice, 5) {
    consider(p);
}
```

**4d. 2-tavun run-lengthit**
```rust
// Lisätty:
if let Some(p) = self.find_any_run_length(slice, 2) {
    consider(p);
}
```

**Syy:** Aiemmin explore haki vain pidemmät mallit (4+ tavua). Lyhyemmät toistuvat mallit (3-byte patterns, 2-byte runs) voivat silti tuottaa hyötyä erityisesti jos ne toistuvat usein.

**Vaikutus:** Explore löytää nyt enemmän mikrooptimointeja, jotka voivat kasvaa hyödyllisiksi toistuvissa sekvensseissä.

---

### 5. Scheduler-optimointi - Aggressiivisempi explore

**Tiedosto:** `src/scheduler.rs`  
**Muutos:** Explore-todennäköisyys 0.7 → 0.8 kun explore on tuottavaa

```rust
// Jos explore on tuottavaa, käytä sitä useammin (parannettu 0.7 -> 0.8)
if stats.gain_per_quota_explore > 0.0 {
    if rng.gen_range(0.0..1.0) < 0.8 {
        return Action::Explore;
    }
}
```

**Syy:** Kun explore tuottaa hyötyä, järjestelmän kannattaa jatkaa oppimista aggressiivisemmin ennen kuin se siirtyy vain hyödyntämään olemassa olevia malleja.

**Vaikutus:** Parempi tasapaino exploration ja exploitation välillä, erityisesti oppimisen alkuvaiheessa.

---

## Odotetut tulokset

Näiden muutosten jälkeen tulokset pitäisi näyttää:

1. **gain_per_quota_explore** ei putoa nollaan syklin 21 jälkeen → jatkuva oppiminen
2. **gain_per_quota_exploit** nousee, kun järjestelmä käyttää kaikkia 150 mallia (ei vain 100)
3. **patterns_count** pysyy tasaisempana ja monipuolisempana (ei vain sanoja)
4. **c_models** laskee tehokkaammin → parempi pakkaussuhde
5. **world_size** kasvaa hitaammin tai saavuttaa tasapainon nopeammin

---

## Seuraavat askeleet

Näiden muutosten jälkeen voit:
- Ajaa järjestelmän uudelleen ja verrata uutta `results.csv` aikaisempaan
- Tarkkailla `gain_per_quota_explore` ja `gain_per_quota_exploit` arvoja
- Harkita `pattern_bank_capacity` kasvattamista 150 → 200, jos kaikki mallit ovat hyödyllisiä
- Säätää `cycle_count % 10` arvoa (esim. % 15 tai % 20) jos sanakirja vie edelleen liikaa tilaa

---

## Testaus

Kaikki muutokset on testattu ja käännetty onnistuneesti:
```bash
cargo check  # ✓ Passed
cargo build --release  # ✓ Passed
```

Voit ajaa järjestelmän komennolla:
```bash
cargo run --release
```

tai suoraan:
```bash
.\target\release\petri_dish.exe
```
