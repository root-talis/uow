/// Marker trait for database platforms.
/// Each platform defines its own transaction type.
pub trait DatabasePlatform: Send + Sync {
    /// The transaction type for this platform.
    type Transaction<'a>: Send;
}
