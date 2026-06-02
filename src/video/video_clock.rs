use std::time::{Duration, Instant};

pub struct VideoClock {
    start_time: Option<Instant>,
    time_base: f64,
}

impl VideoClock {
    pub fn new(time_base: f64) -> Self {
        Self {
            start_time: None,
            time_base,
        }
    }

    pub fn start(&mut self) {
        self.start_time = Some(Instant::now());
    }

    pub fn time_till_next_frame(&mut self, pts: i64) -> Option<Duration> {
        if self.start_time.is_none() {
            self.start();
        }
        #[allow(clippy::cast_precision_loss)]
        let target_time_secs = pts as f64 * self.time_base;
        let current_time_secs = self.start_time.unwrap().elapsed().as_secs_f64();
        let diff = target_time_secs - current_time_secs;

        if diff > 0.0 {
            Some(Duration::from_secs_f64(diff))
        } else {
            None
        }
    }
}
