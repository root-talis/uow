use chrono::{DateTime, Utc};

/// A message DTO representing an email with its creation timestamp.
/// This is not a domain entity; it lives in the application layer.
#[derive(Clone)]
pub struct Message {
    pub message: String,
    pub created_at: DateTime<Utc>,
}
