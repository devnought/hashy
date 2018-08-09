use pbr::ProgressBar;
use std::io::{self, Stderr};
use threadpool::ThreadPool;

enum ProgressType {
    Inc(ProgressBar<Stderr>),
    Tick(ProgressBar<Stderr>),
    None,
}

pub struct Progress<'a> {
    count: u64,
    success: u64,
    enabled: bool,
    pb: ProgressType,
    pool: &'a ThreadPool,
}

impl<'a> Progress<'a> {
    pub fn new(pool: &'a ThreadPool) -> Self {
        Self {
            count: 0,
            success: 0,
            enabled: false,
            pb: ProgressType::None,
            pool,
        }
    }

    pub fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }

    pub fn build_status(&mut self) {
        if !self.enabled {
            return;
        }

        let mut p = ProgressBar::on(io::stderr(), 0);
        p.show_speed = false;
        p.show_percent = false;
        p.show_counter = false;
        p.show_time_left = false;
        p.message("Processing files");

        self.pb = ProgressType::Tick(p);
    }

    pub fn build_bar(&mut self, total: u64) {
        if !self.enabled {
            return;
        }

        let mut p = ProgressBar::on(io::stderr(), total);
        p.set(self.count);
        p.show_speed = false;
        p.show_time_left = false;
        //p.format("███░█");

        self.pb = ProgressType::Inc(p);
    }

    pub fn inc(&mut self) {
        self.count += 1;

        match self.pb {
            ProgressType::None => {}
            ProgressType::Inc(ref mut p) => {
                p.inc();
            }
            ProgressType::Tick(ref mut p) => {
                p.message(&format!(
                    "Processing files ({}) ({} : {} / {})",
                    self.count,
                    self.pool.queued_count(),
                    self.pool.active_count(),
                    self.pool.max_count()
                ));
                p.tick();
            }
        }
    }

    pub fn inc_success(&mut self) {
        self.inc();
        self.success += 1;
    }
}
