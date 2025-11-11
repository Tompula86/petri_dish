# Suunnitelma: Petrimalja älykkyydelle — tiedon tiivistäminen paineen alla

Tämä dokumentti kuvaa tavoitteen, arkkitehtuurin ja etenemissuunnitelman järjestelmälle, joka ei yritä "olla älykäs" valmiiksi, vaan ansaitsee älykkyytensä oppimalla tiivistämään tietoa. Kyse ei ole pakkausohjelmasta per se, vaan digitaalisesta epistemologian kokeesta: kuinka hyvin rajallisessa tilassa elävä toimija keksii itsenäisesti malleja ("totuuksia"), jotka selittävät dataa lyhyemmin kuin data itse.


## Ydinfilosofia

- Älykkyys ei ole datan määrän funktio, vaan sen, kuinka tehokkaasti dataa voidaan käsitellä.
- "Totuus" määritellään tässä: mikä tahansa malli/operaattori, joka kuvaa dataa lyhyemmin kuin data itse (MDL-henkinen ajattelu).
- Palkitaan järjestelmää, joka keksii itse pakkaamisen — ei vain käytä ennalta annettuja algoritmeja.
- 100 kB haaste: Kuinka älykkääksi/tahokkaaksi 100 kilotavua voi kehittyä?


## Käsitteet ja sanasto

- World: rajattu muistialue (esim. 100 kB), jossa data elää ja johon transformaatioita sovelletaan.
- Solver (Ratkaisija): toimija, jolla on oma sisäinen tila ja pieni "aivot"-muisti; se etsii, keksii ja soveltaa malleja.
- Pattern (Malli): esitys tavasta kuvata jokin datan rakenne lyhyemmin (esim. toisto, sanakirja, säännöllinen rakenne tai meta-sääntö).
- Operator (Operaattori): toimintauskomalli, joka muuntaa dataa ja jolla on kuvauskustannus ja hyödynnettävyyden mitta.
- PatternBank (Tietopankki): Solverin oppimisen kertyvä muisti (rajallinen; kilpailevat säännöt).
- Evaluator (Arvioija): mittaa kokonaiskustannuksen ja hyväksyy vain muutokset, jotka parantavat nettoa.


## Sopimus (mini-kontraktti)

- Input: Binaaridata Worldissä; deterministinen siemen (seed); resurssirajat (aika/quota, tila-budgetit).
- Output: Parannettu kuvauksen kokonaismitta (Data|Mallit), löydetyt mallit ja sovellushistoria.
- Menestyskriteeri: Nettokustannus pienenee: C_total = C(modelit) + C(jäännösdata). Pienempi on parempi.
- Virhetilat: Aikaraja/quota loppuu; malli ei kelpaa (ei vähennä kustannusta); PatternBank täynnä (käynnistä unohtaminen).


## Rajoitteet ja reunaehdot

- Ei ulkoisia ML-kirjastoja tai valmista pakkausta – heuristiikka ja meta-heuristiikka sallittuja.
- Reprodusoitavuus: deterministiset kokeet siemenellä.
- Budjetit: World oletus 10 kB (ympäristömuuttujilla säädettävissä); Solver "aivot" esim. 16–64 kB (tarkennetaan); prosessointiquota (aika/iteraatiot) run-kierrosta kohti.
- Turvallisuus: Operaattorit eivät saa korruptoida Worldin rakennetta; muutokset ovat "transaktioita" palautusmahdollisuudella.


## Arkkitehtuuri (korkean tason kuva)

- World: byte-viipale + metatieto (kustannus, segmentit, jäännösmerkinnät); oletus 10 kB, säädettävissä ympäristömuuttujilla.
- Feeder (Syöttäjä): tuo sisään uutta dataa virrana konfiguroitulla nopeudella ja skaalaa feed_ratea automaattisesti vapaan tilan mukaan.
- FocusWindow: dynaaminen rullaava ikkuna (128 B → min(PETRI_WINDOW_FRACTION · world_limit, world_limit)), jonka koko kasvaa eksponentiaalisesti nolla-hyötyjaksojen perusteella ja joka kattaa koko maailman deterministisesti.
- Solver (Agentti): pitää PatternBankia, Stats-tilaa ja Scheduleria; pääsilmukka interleavoi exploit/explore/meta/forget sekä hallitsee FocusWindow-tilaa.
- Evaluator: kustannusfunktio (MDL), joka hyväksyy vain nettohyötyä tuottavat muutokset.
- Scheduler: jakaa quota-budjetin strategioiden kesken, priorisoi ShiftWindow-rullausta kun hyöty pysähtyy ja tukee meta-oppimista.
- PatternBank: rajallinen kilpailullinen muisti (RunLength, BackRef, Dictionary, GrammarRule, GeneralizedBackRef, ...).
- Operator DSL/IR: kompakti bytemuotoinen esitys operaattoreille; meta-taso voi muodostaa uusia Operaattori-perheitä (Dictionary-sanat, Grammar-säännöt, BackRef-yleistykset).


C_total = C(models) + C(residual)

Missä:
- C(models): mallit + niiden koodaus (ID, parametrit, lukumäärä, järjestys)
- C(residual): Worldin jäljelle jäävän tiedon koodaus yksinkertaisella baseline-koodauksella

Hyväksy malli pääsääntöisesti, jos Δ = C_before − C_after − C(model_delta) > 0 (turvamarginaali epsilon > 0).

Ajallinen näkökulma ja priorisointi: Feederin luoma paine tekee ajasta arvokasta. Käytännön valinta tehdään prioriteettifunktiolla, joka suosii suurta hyötyä per käytetty quota:

- score ≈ E[gain_bytes] / E[cost_quota] (sekä mahdollinen tuoreus/paikallisuus-termin lisäys)
- vaihtoehtoisesti monikriteerinen tavoite: minimoi J = C_total + λ · T, missä T on käytetty quota; λ säätyy paineen mukaan

Riskibudjetti: rajattu osuus kierroksesta voi hyväksyä myös pieniä negatiivisia Δ-arvoja (Δ > −ε_risk) tutkimisen jatkamiseksi, ks. erillinen kohta.


## Solver agentiksi: rakenne ja elinkaari

- Solver live(world):
  0) Päivitä paine: tarkista Feederin sisääntulovauhti ja vapaana oleva tila; säädä λ ja strategia-budjetit.
  1) Exploit (ikkunassa): käy known_patterns läpi prioriteettijärjestyksessä (gain/quota, tuoreus, paikallisuus) ja sovella nopea skannaus nykyisessä FocusWindow-alueessa.
  2) Explore (ikkunassa + lähiympäristö): kohdennettu haku (toistot, n-grammit, differenssit, XOR-maskit) rajoitetulla säteellä; harkittu ikkunan siirto, jos score paranee.
  3) Meta-learn: etsi malleja malleista PatternBankissa; yleistykset (esim. toisto-operaattorin abstraktointi).
  4) Forget: jos PatternBank täynnä, poista vähiten hyödyllinen (LFU/"least saved bytes"/ikäpainotus).
  5) Risk-askel (valinnainen): käytä pientä riskibudjettia negatiivisen Δ:n kokeiluihin lupaavilla alueilla.
  6) Siirrä FocusWindow systemaattisesti (ShiftWindow) seuraavaan stride-positioon; kasvata ikkunaa kun nolla-hyötyjakso ylittyy.
  7) Päivitä stats ja quota-budjetit; Feeder työntää sisään uutta dataa; jos tila loppuu, siirry "hätätilaan" (nopeat, korkea gain/quota -operaatiot).


### Rust-skeema (suuntaa antava)

```rust
struct Solver {
    known_patterns: Vec<Pattern>,
    processing_quota: u32,
    stats: Stats,
}

impl Solver {
    fn live(&mut self, world: &mut World) {
        self.exploit(world);
        self.explore(world);
        self.meta_learn();
        self.forget_if_needed();
    }
}
```

Pattern voi olla enum + parametrit; Operator voidaan erottaa erilliseksi IR:ksi.


## Dynaaminen paine: Feeder ja ajan arvo

- Feeder tuo uutta dataa jokaisen live()-syklin päätteeksi (perusnopeus 200 B/sykli, laskee automaattisesti jos muistipaine kasvaa).
- Konfigurointi: PETRI_FEED_RATE ja PETRI_WORLD_LIMIT -ympäristömuuttujilla voi nostaa/laskaa perusnopeutta ja worldin muistikattoa kokeita varten.
- Jos World olisi täyttymässä, Solver priorisoi nopeasti sovellettavat mallit, jotka vapauttavat tilaa ennen ylivuotoa.
- Ajan arvo: ajankäyttö on eksplisiittinen kustannus. Scheduler nostaa scorea, joka maksimoi E[gain]/E(quota). Paineen kasvaessa λ kasvaa (aika on kalliimpaa), mikä ohjaa kohti nopeita voittoja.
- Backpressure: jos edes nopeilla voitoilla ei pysytä tasapainossa, Feeder voidaan hidastaa tai pysäyttää kokeellisesti (konfiguroitava politiikka).


## Tarkennusikkuna (Focus Window) ja paikallisuus

- Ikkunan koko: lähtö 128 B, kasvaa eksponentiaalisesti (×1.5) aina kun useampi sykli tuottaa nolla-hyödyn; maksimi = min(PETRI_WINDOW_FRACTION · world_limit, world_limit).
- Stride ≈ window_size/2, jolloin rullaava ShiftWindow kattaa koko datan deterministisesti (wrap-around) ennen palaamista alkuun.
- Kustannukset: seek ja read kuluttavat quota-yksiköitä; ShiftWindow-toiminto raportoidaan stats.quota_spent_seek-mittarin kautta.
- Strategiat: coarse scan → hot spot → fine scan; tuore data korjataan nopeasti, mutta systemaattinen rullaus takaa myös vanhan datan revisiitin.
- Paineesta riippuva kasvun rajoitus: kun world muistista käytössä suuri osuus, ikkuna kasvaa vain jos PatternBank ei löydä uusia voittoja; muuten koko pidetään kompaktina.
- Emergentti käytös: isot mallit löytyvät rullauksen seurauksena, pienet mallit käsitellään tailissa nopeasti.


## Riskibudjetti: pakeneminen paikallisista minimeistä

- Pieni kokeilullinen budjetti sallii hyväksymään Δ > −ε_risk -muutoksia, jos odotettu jatkohyöty on korkea.
- Vaihtoehto: annealing-tyylinen hyväksyntä todennäköisyydellä p = exp(−max(0, −Δ)/T); T laskee ajan myötä tai paineen kasvaessa.
- Turvakaista: riskibudjetti on osuus kokonaisquotasta (esim. ≤ 5–10%), ettei järjestelmä degeneroidu.


## Strateginen quota-allokointi (oppimaan oppiminen)

- Solver.stats seuraa tuottavuutta: gain_per_quota strategioittain (exploit, explore: [repeat, n-gram, delta, ...], meta, forget-overhead).
- Scheduler jakaa budjetit pehmeästi esim. softmaxilla viimeaikaisten tuottavuuksien yli tai bandiitti-tyylisellä UCB:llä.
- Nopeasti sopeutuva: kun Feeder nostaa entropiaa, explore saa lisää budjettia; kun maailma “kypsyy”, meta saa etusijan.


## Pattern ja Operator – esitysmuoto

- Pattern { id, op: Operator, params, footprint, encoded_cost, last_gain, hits }
- Operator (Enum): Literal, Repeat(k), Dictionary(id), RunLength, Delta(base), Xor(mask), Periodic(period), Grammar(rule), Meta(…)
- IR/DSL: kompakti bytemuotoinen esitys: [OPCODE | PARAMS] → mahdollistaa meta-operaattorien syntymisen (operaattori, joka tuottaa toisia operaattoreita)
- Matching-API: estimate_gain(world) → Option<Gain>; apply(world) → Patch; rollback(world, Patch)


## Hyödyntäminen vs. Tutkiminen

- Exploit: O(log N)–O(N) skannit priorisoiduille malleille; early-exit; turvakaistat (ei päällekkäisiä sovelluksia ellei hyväksytty yhdistäjä).
- Explore: 
  - Testi 1: toistot ja run-length-rypyt
  - Testi 2: lyhyet mallit (n-grammit, periodiciteetti)
  - Testi 3: delta/xor suhteessa lähinaapureihin tai globaaliin baselineen
  - Testi 4: pieni grammar induction (esim. Sequitur-tyyppinen sääntöjen muodostus)
- Uuden mallin hyväksyntä: nettohyöty > kynnys; lisää PatternBankiin, jos tila sallii, muuten käynnistä forget.

Priorisointi käytännössä: valitse seuraava toimenpide argmax score(candidate), missä score painottaa gain/quota, ikkunaläheisyyttä ja tuoreutta; ota riskibudjetista harvoin negatiivinen Δ.


## Meta-mallit ja yleistys (Vaihe 4)

- PatternBankiin kohdistuva analyysi: etsi rakenteellisia yhtäläisyyksiä (esim. "xxx" ja "yyy" → Repeat(c,3)).
- Meta-operaattorin synty: kun havaitaan, että joukko malleja on instansseja samasta perheestä, tuota yleistysoperaattori ja korvaa spesifit instanssit.
- Refaktorointi: vähennä C(models) yhdistämällä redundantit säännöt yleistykseksi.


## Unohtaminen ja resurssioptimointi

- Kapasiteetti: esim. 100 patternia PatternBankissa.
- Poistopolitiikka: LFU + "saved-bytes" + ikä (WLRU). Tavoite: maksimoida tuleva odotettu hyöty per muistibitti.
- Säännöllinen saneeraus: jos malli ei enää tuota nettohyötyä nykyisessä World-tilassa, alenna prioriteettia tai poista.

Feeder-informoitu unohtaminen: kun tila on kriittinen, priorisoi säilytettäviksi mallit, jotka ovat nopeasti sovellettavia ja tuottavat suurta gain/quota -arvoa.


## Arviointi ja mittarit

- Compression-equivalent: C_before/C_after (suuntaa antava, ei varsinainen pakkaus)
- Surprise score: uusien malliperheiden ilmaantuminen vs. ennalta määritetyt baseline-operatorit
- Discovery rate: kelvollisten mallien löytäminen/quota-yksikkö
- Reuse ratio: kuinka moni oppitu malli hyödynnetään myöhemmin
- Generality: meta-mallien kattavuus useille dataseteille
- Stability: tulosten hajonta eri siemenillä
- Time-aware: bytes saved per second / per quota, backlog growth vs. feeder rate, overflow-incident rate, ikkunan liikkumiskustannus/osuus, riskibudjetin ROI.

### Onnistumisen Kojelauta (Dashboard)

Mittaukset jaetaan kahteen pääulottuvuuteen: "Tieto" (PatternBankin laatu) ja "Osaaminen" (Solverin toiminnallinen kompetenssi paineessa).

#### Tieto (Knowledge)

1. Abstraktion Taso (C(models) trendi)
  - Seurataan C(models) erikseen.
  - Odotettu vaiheistus: alku → kasvu (paljon yksittäisiä malleja) → romahdus (meta-malli yleistää ne) → tasainen matala plato.
  - Signaali onnistumisesta: merkittävä suhteellinen pudotus C(models) ilman että C(residual) kasvaa.

2. Mallien Voima (Generality Score)
  - Määritelmä: generality = saved_bytes / C(model).
  - Raportoidaan keskiarvo, mediaani ja top-k.
  - Käytetään unohtamispäätöksissä ja priorisointiin.

3. Meta-yhdistämisen Tehokkuus
  - (sum(original_models_cost) - cost(meta_model_family)) / time_spent_meta.
  - Näyttää meta-learn -vaiheen ROI:n.

#### Osaaminen (Skill)

4. Sopeutumisnopeus (Adaptation Speed)
  - Testiprotokolla alla erillisessä osiossa.
  - Mittari: palautumissyklit (syklien määrä siihen, että gain_per_quota palautuu ≥ α · baseline).
  - Tallennetaan myös strategia-allokointien muutosviive (ms / syklit ennen budjetin reallokointia).

5. Strateginen Resurssien Allokointi
  - Divergenssi optimaalisesta (esim. KL-divergence todellisen quota-jakauman ja dynaamisen tuottavuus-softmaxin välillä).
  - Nopeus: aika siihen, että allokointi siirtyy > β osuudella explorationiin kriisissä.

6. Ikkunatehokkuus
  - window_efficiency = (bytes_saved_in_window / total_bytes_saved).
  - Liialliset seekit: seek_cost_share = (quota_spent_seek / total_quota). Alhainen on hyvä.

7. Riskibudjetin ROI
  - risk_roi = (net_bytes_saved_due_to_risk - net_bytes_lost_in_failed_risks) / quota_spent_risk.
  - Riskin varoitus: jos ROI < 0 useamman peräkkäisen jakson ajan → riskibudjettia pienennetään automaattisesti.

8. Bandit Regret (Strategia-valinnat)
  - Formuloidaan eksplorointistrategiat toimintovaihtoehtoina; regret = Σ (best_gain_per_quota - chosen_gain_per_quota).
  - Aleneva trendi osoittaa oppimista oppimisessa.

9. Backpressure Stability
  - backlog_delta = feeder_backlog_change / cycle.
  - stable jos |backlog_delta| < ε pitkällä ikkunalla.

10. Overflow Incident Rate
   - (overflow_events / cycles). Tavoite lähestyä nollaa.

#### Koontimetriikat

- Knowledge Maturity Index (KMI): f(C(models)_norm, avg_generality, meta_efficiency)
- Skill Responsiveness Index (SRI): f(adaptation_speed_norm, strategy_shift_latency_norm, regret_norm)
- System Pressure Health (SPH): f(backpressure_stability, overflow_incidents, window_efficiency)

Näistä voidaan tuottaa radiaalikaavio tai trendi dashboardissa.

### Sopeutumisnopeus – Testiprotokolla

1. Valmistelu
  - Alusta World datalla A (esim. pitkät tasaiset toistot: 000000...).
  - Aja solveria kunnes steady-state: gain_per_quota(Exploit) ≥ g_A_baseline ja C(models) stabiloitui.
2. Shokki
  - Vaihda Feeder syöttämään dataa B (erilainen rakenne: esim. 123123..., tai korkeampi entropia + periodinen malli).
3. Mittaus
  - Tallenna sykli t0 kun muutos tapahtuu.
  - Seuraa jokaisessa syklissä: gain_per_quota eksploitille, explore-alloc%, uusi löydetty pattern_family, adaptation_phase_flag.
4. Palautumisen määritelmä
  - Palautunut, kun gain_per_quota ≥ α · g_B_baseline (baseline mitataan erillisellä ajolla, jossa pelkkä data B alusta asti).
  - adaptation_cycles = t_recovery - t0.
5. Lisämittarit
  - discard_latency: syklit vanhan mallin poistoon (OP_REPEAT_A).
  - first_new_model_latency: syklit ensimmäiseen uuteen general patterniin B:stä.
  - quota_realloc_latency: syklit siihen, että explore_alloc% ≥ γ (esim. 80%).
6. Pass-kriteerit (esimerkkiluvut, iteratiivisesti tarkennettavat):
  - adaptation_cycles ≤ 0.25 · baseline_cycles_A (nopeampi kuin oppiminen alusta).
  - quota_realloc_latency ≤ 3 syklin ikkunaa.
  - discard_latency ≤ 5 sykliä.
  - risk_roi ≥ 0 testin aikana (riski ei syö nettohyötyä).
7. Raportointi
  - Tuota aikaviiva (timeline): {cycle, gain_per_quota, explore_alloc%, new_models_count, C(models)}.
  - Visualisoi paikka jossa C(models) mahdollisesti kasvaa ennen romahdusta meta-yhdistämisen kautta.

### Instrumentointi ja keruu

- Jokainen live()-kutsu palauttaa StatsSnapshot:
  { cycle_id, quota_spent: {exploit, explore_repeat, explore_ngram, meta, risk, seek}, gain_bytes: {...}, C_models, C_residual, backlog, window_pos, overflow_flag }
- Aggregaatiokerros laskee johdettuja metriikoita (ROI:t, regret, tehokkuus).
- Dumpataan NDJSON tai binäärihistoria offline-analyysiin.

### Kokeelliset skenaariot

1. Virtaava monotoninen → shokki entropiaan
2. Vaihtuva periodi (muuttuva jakso) → testaa meta-mallin kykyä yleistää period length.
3. Purskeinen syöttö → backpressure-resilienssi ja emergency_relief.
4. Kohina + rakenteen esiintyminen satunnaisesti → window strategia (hot spot revisit).
5. Adversariaalinen sekvenssi, joka yrittää houkutella huonoihin malleihin → riskibudjetin hallinta.

### Hyväksymiskriteerit kokonaisuudelle

- KMI nousee > θ_knowledge ja stabiloituu.
- SRI nousee > θ_skill testisarjoissa.
- SPH pysyy > θ_health kaikissa paineprofiileissa (ei jatkuvaa overflowia).
- Overflow Incident Rate < 0.01 pitkällä juoksulla.
- Bandit regret trendaa laskevaan eksponentiaaliseen suuntaan (oppiminen meta-strategiassa).

### Laajennettavat tulevat metriikat

- Concept Drift Sensitivity: mittaa pienen, jatkuvan muutoksen havaitsemisviive.
- Pattern Lifespan Distribution: histogrammi siitä, kuinka pitkään mallit pysyvät hyödyllisinä.
- Cross-Dataset Transfer Gain: meta-mallien välitön hyöty uudessa datassa ilman retrainia.


## Testiaineistot (kuratoitu valikoima)

- Synteettiset: 
  - pitkät toistot, periodiset kuviot, sahalaita/delta, XOR-maskatut kuviot
  - grammari: a^n b^n, yksinkertaiset sääntöperheet
- Meluisat: toistoon sekoitettu kohina
- Reaaliset: pieni teksti, heksadumppi, pikkutason binääri
- Virtaava: saumaton syöttö (Feeder) erilaisilla jakautumilla (tasainen, purskeinen, trendikäs)


## Reunatapaukset ja suojaukset

- Tyhjä/lyhyt data: palautuvat oletusbaselineen
- Adversariaalinen data: estä mallit, joiden hyöty syntyy pelkästä ylioppimisesta paikalliseen konfiguraatioon
- Päällekkäiset sovellukset: transaktio- ja yhdistämissäännöt
- Aikarajat: keskeytä turvallisesti ja tallenna tila
- Feeder-ylikuormitus: jos syöttönopeus > kestokyky, ota käyttöön backpressure tai degradeerauspolitiikka (esim. väliaikainen riskibudjetin nosto, priorisoi nopeat voitot, tai pysäytä Feeder).


## Roadmap (vaiheet ja hyväksymiskriteerit)

- Vaihe 0: Bootstrap
  - Tavoite: projekti run/CI; World + Evaluator baseline
  - Kriteeri: C_total lasketaan deterministisesti pieneen dataan

- Vaihe 1: Yksinkertainen haku (ei agenttia)
  - Tavoite: Testi 1–2 (toistot, n-grammit) ja hyväksyntälogiikka
  - Kriteeri: C_total pienenee ainakin yhdellä datasetillä

- Vaihe 1.5: Feeder perusmalli
  - Tavoite: virrasyöttö konfiguroitavalla nopeudella; backpressure-kytkin
  - Kriteeri: järjestelmä ei ylitä muistia, jos perusyksinkertaiset mallit riittävät tasaamaan virran

- Vaihe 2: Operator-IR ja Patch-rajapinta
  - Tavoite: operaattorit bytemuodossa; apply/rollback
  - Kriteeri: kaksi erilaista operaattoria toimii yhdessä

- Vaihe 3: Solver agentiksi
  - 3.1: live(), exploit/explore; tyhmä agentti
  - 3.2: PatternBank + oppiminen (lisää löydetyt mallit pankkiin)
  - Kriteeri: toistuva ajo nopeutuu ja paranee, koska exploit hyödyntää opittuja malleja

- Vaihe 2.5: FocusWindow 2.1
  - Tavoite: rullaava, deterministinen ikkunaskannaus (stride ≈ window/2, wrap-around) joka kasvaa eksponentiaalisesti nolla-hyötyjaksojen perusteella ja kunnioittaa PETRI_WINDOW_FRACTION -rajaa.
  - Kriteeri: jokainen tavupositio käydään läpi ≤2·(world_len/window_stride) syklissä, ja ikkuna laajenee itsestään kun exploit/explore-gain pysyy nollassa ≥3 sykliä.

- Vaihe 2.6: Dictionary 2.0
  - Tavoite: jatkuva n-grammien ingestion koko worldista, sanakirjan ylläpito (cap 256 sanaa), sekä exploit/explore -polku joka käyttää pisimpiä sanoja ensin.
  - Kriteeri: PatternBank sisältää Dictionary-malleja, jotka tuottavat nettohyötyä; stats.gain_per_quota_exploit ei jää nollaan kun data sisältää toistuvia sanoja.

- Vaihe 2.7: Grammar Induction
  - Tavoite: explore_meta kerää operaattorivirran, tunnistaa toistuvat operaattorijonot ja luo Operator::GrammarRule-säännöt + soveltaa niitä paikallisesti.
  - Kriteeri: vähintään yksi GrammarRule tallentuu PatternBankiin ja tuottaa säästöä testidatassa; meta-tilastot raportoivat uuden sääntöperheen syntymisen.

- Vaihe 4: Meta-mallit ja unohtaminen
  - 4.1: meta-learn PatternBankista, yleistysoperaattorit
  - 4.2: kapasiteettiraja ja valikoiva unohtaminen
  - Kriteeri: C(models) pienenee refaktoroinnin seurauksena; kokonaishyöty kasvaa

- Vaihe 4.3: Strateginen quota-allokointi
  - Tavoite: stats.gain_per_quota ja dynaaminen Scheduler (softmax/UCB)
  - Kriteeri: budjetit siirtyvät automaattisesti tuottavampiin strategioihin eri dataseteillä

- Vaihe 4.4: Riskibudjetti
  - Tavoite: kontrolloitu negatiivisen Δ:n hyväksyntä + mahdollinen annealing
  - Kriteeri: järjestelmä välttää jumittumisen; pitkän aikavälin nettohyöty kasvaa ilman epävakautta

- Vaihe 5: Turnajaiset ja auto-curriculum
  - Generoi datasettihaasteita, mittaa generality/stability

- Vaihe 6: CLI/visualisaatio + julkaisu

- Vaihe 7: Kojelauta ja mittauskerros
  - Tavoite: StatsSnapshot, NDJSON-keruu, radiaalikaavio KMI/SRI/SPH, adaptation test runner.
  - Kriteeri: Kaikki päämetriikat tallentuvat ja sopeutumisnopeus-testin raportti generoituu automaattisesti.


## Pienin toimiva esimerkki (MVP-polku)

1) World = bytes[...]
2) Evaluator: C_total = |world| (baseline) aluksi
3) Explore: etsi run-length-rypyt; jos löytyy, lisää Pattern::RunLength(k)
4) Evaluator laskee Δ; jos > 0, apply ja tallenna PatternBankiin
5) Seuraavalla ajolla exploit käyttää samaa Patternia suoraan


## Riskit ja torjunta

- Hakuräjähdys: priorisointi, quota, beam search
- Degeneroituvat operaattorit: säännöt ja minimihyötykynnys; sandbox-sovellus
- Illuusio parannuksesta: A/B-testaus eri siemenillä; validointidatasetit
- Ylioppiminen: generality-mittari ja risti-evaluaatio


## Ei-tavoitteet (ainakaan nyt)

- Keskustelubotti tai kielimalli
- Ulkoisen ML:n käyttö sivuuttamaan ongelmaa
- Maksimaalinen pakkaussuhde; tavoitteena on oppiminen ja yleistys


## Toteutusvinkit ja käytännöt

- Selkeä eriytys: pattern-match vs. kustannuslaskenta vs. sovellus
- Läpinäkyvyys: jokaiselle päätökselle perustelu (gain, cost)
- Fuzzer/satunnaistestaus Patch/rollback-turvallisuudelle
- Profilointi: mittaa aikaa/byte ja hyöty/byte jokaiselle operaattorille


## Seuraavat konkreettiset askeleet

- Luo moduulit: world.rs, solver.rs, pattern.rs, operator.rs, evaluator.rs
- Kirjoita kustannusfunktiolle yksikkötestit (happy path + boundary)
- Implementoi Repeat/RunLength operatorit ja Patch-rajapinta
- Lisää Solver::live() ilman meta-oppeja, sitten PatternBank
- Lisää Feeder + FocusWindow -rungot ja yksinkertainen Scheduler, joka käyttää gain/quota -prioriteettia
- Mittarit: tulosta hyödyt ja ajan käyttö ajon lopuksi


## Pseudokoodi: live-silmukka (Feeder + FocusWindow + dynaaminen Scheduler)

```rust
fn live(&mut self, world: &mut World, feeder: &mut Feeder) {
  self.scheduler.reweigh(&self.stats, world.pressure(), feeder.rate());
  let mut quota = self.processing_quota;

  while quota > 0 {
    // 1) Exploit in-window
    quota -= self.exploit_in_window(world);

    // 2) Explore near window (may propose window shift)
    if self.scheduler.should_explore() {
      quota -= self.explore_local(world);
    }

    // 3) Meta-learn (budgeted)
    if self.scheduler.should_meta() {
      quota -= self.meta_learn();
    }

    // 4) Forget if needed
    if self.pattern_bank.is_full() { self.forget_if_needed(); }

    // 5) Risk step (rare)
    if self.scheduler.allow_risk() { quota -= self.risky_probe(world); }

  // 6) Window management
  self.sync_focus_window(world);
  }

  // 7) Intake new data; emergency mode if near overflow
  feeder.feed(world);
  if world.near_overflow() { self.emergency_relief(world); }
}
```


---

