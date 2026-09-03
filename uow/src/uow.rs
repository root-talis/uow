use crate::platform::DatabasePlatform;

/// Marker trait for Unit of work, tied to a specific database platform.
pub trait UnitOfWork<P: DatabasePlatform>: Send {}
