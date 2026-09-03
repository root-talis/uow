use chrono::Utc;

use crate::application::ports::clock::Clock;

/// Real system clock implementation.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        Utc::now()
    }
}
