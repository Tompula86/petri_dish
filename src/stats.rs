/// Stats: Tilastot Solverin suorituskyvystä ja oppimisesta
#[derive(Debug, Clone)]
pub struct Stats {
    /// Gain per quota exploit-strategialle (tavua säästetty / quota käytetty)
    pub gain_per_quota_exploit: f64,
    /// Gain per quota explore-strategialle
    pub gain_per_quota_explore: f64,
    /// Gain per quota meta-learning strategialle
    pub gain_per_quota_meta: f64,
    /// C(models): Mallien koodauskustannus
    pub c_models: usize,
    /// C(residual): Jäljellä olevan datan kustannus
    pub c_residual: usize,
    /// Yhteensä kulutettu quota tässä syklissä
    pub quota_spent_exploit: u32,
    pub quota_spent_explore: u32,
    pub quota_spent_meta: u32,
    pub quota_spent_seek: u32,
    /// Yhteensä säästetty tavuja tässä syklissä
    pub bytes_saved_exploit: i32,
    pub bytes_saved_explore: i32,
    pub bytes_saved_meta: i32,
}

impl Stats {
    pub fn new() -> Self {
        Stats {
            gain_per_quota_exploit: 0.0,
            gain_per_quota_explore: 0.0,
            gain_per_quota_meta: 0.0,
            c_models: 0,
            c_residual: 0,
            quota_spent_exploit: 0,
            quota_spent_explore: 0,
            quota_spent_meta: 0,
            quota_spent_seek: 0,
            bytes_saved_exploit: 0,
            bytes_saved_explore: 0,
            bytes_saved_meta: 0,
        }
    }

    /// Nollaa syklin tilastot (kutsutaan jokaisen live()-kierroksen alussa)
    pub fn reset_cycle(&mut self) {
        self.quota_spent_exploit = 0;
        self.quota_spent_explore = 0;
        self.quota_spent_meta = 0;
        self.quota_spent_seek = 0;
        self.bytes_saved_exploit = 0;
        self.bytes_saved_explore = 0;
        self.bytes_saved_meta = 0;
    }

    /// Päivitä exploit-tilastot
    pub fn record_exploit(&mut self, quota_spent: u32, bytes_saved: i32) {
        self.quota_spent_exploit += quota_spent;
        self.bytes_saved_exploit += bytes_saved;
        self.update_gain_per_quota();
    }

    /// Päivitä explore-tilastot
    pub fn record_explore(&mut self, quota_spent: u32, bytes_saved: i32) {
        self.quota_spent_explore += quota_spent;
        self.bytes_saved_explore += bytes_saved;
        self.update_gain_per_quota();
    }

    /// Päivitä meta-learning-tilastot
    pub fn record_meta(&mut self, quota_spent: u32, bytes_saved: i32) {
        self.quota_spent_meta += quota_spent;
        self.bytes_saved_meta += bytes_saved;
        self.update_gain_per_quota();
    }

    /// Laske gain/quota-suhde
    fn update_gain_per_quota(&mut self) {
        if self.quota_spent_exploit > 0 {
            self.gain_per_quota_exploit =
                self.bytes_saved_exploit as f64 / self.quota_spent_exploit as f64;
        }
        if self.quota_spent_explore > 0 {
            self.gain_per_quota_explore =
                self.bytes_saved_explore as f64 / self.quota_spent_explore as f64;
        }
        if self.quota_spent_meta > 0 {
            self.gain_per_quota_meta = self.bytes_saved_meta as f64 / self.quota_spent_meta as f64;
        }
    }

    /// Päivitä seek-tilastot (ikkunan siirto)
    pub fn record_seek(&mut self, quota_spent: u32) {
        self.quota_spent_seek += quota_spent;
    }

    /// Päivitä kustannuskomponentit
    pub fn update_costs(&mut self, c_models: usize, c_residual: usize) {
        self.c_models = c_models;
        self.c_residual = c_residual;
    }

    /// Kokonaishyöty tästä syklistä
    pub fn total_gain(&self) -> i32 {
        self.bytes_saved_exploit + self.bytes_saved_explore + self.bytes_saved_meta
    }

    /// Kokonaisquota käytetty tässä syklissä
    pub fn total_quota_spent(&self) -> u32 {
        self.quota_spent_exploit
            + self.quota_spent_explore
            + self.quota_spent_meta
            + self.quota_spent_seek
    }
}
