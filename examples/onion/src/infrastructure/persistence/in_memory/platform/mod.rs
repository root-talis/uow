use uow::DatabasePlatform;
use transaction::InMemoryTransaction;

pub mod store;
pub mod transaction;

/// In-memory database platform for testing.
pub struct InMemoryPlatform;

impl DatabasePlatform for InMemoryPlatform {
    type Transaction<'a> = InMemoryTransaction;
}
