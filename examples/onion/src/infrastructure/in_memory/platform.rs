use uow::DatabasePlatform;
use crate::infrastructure::in_memory::transaction::InMemoryTransaction;

/// In-memory database platform for testing.
pub struct InMemoryPlatform;

impl DatabasePlatform for InMemoryPlatform {
    type Transaction = InMemoryTransaction;
}
