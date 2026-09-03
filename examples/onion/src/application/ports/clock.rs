use chrono::{DateTime, Utc};

/// Trait for providing the current time.
/// Implementations can be real (system clock) or test doubles.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}
