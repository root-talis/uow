use ::uow::DatabasePlatform;
use transaction::SqliteTransaction;

pub mod transaction;
pub mod uow;

/// Sqlite database platform for testing.
pub struct SqlitePlatform;

impl DatabasePlatform for SqlitePlatform {
    type Transaction<'a> = SqliteTransaction<'a>;
}
