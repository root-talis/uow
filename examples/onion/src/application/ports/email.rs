use uow::{DatabasePlatform, UnitOfWork};

/// Outbox writer that sends emails within a unit of work.
/// Parameterized by both the database platform and the concrete unit of work type
/// to guarantee at compile time that they are compatible.
pub trait EmailTransactionalOutboxWriter<P: DatabasePlatform, UoW: UnitOfWork<P>>: Send + Sync {
    /// Sends an email using the given unit of work.
    async fn send_email(&self, uow: &mut UoW, msg: &str) -> Result<(), String>;
}

/// Outbox reader that dumps emails that were thrown at it.
pub trait EmailTransactionalOutboxReader<P: DatabasePlatform, UoW: UnitOfWork<P>>: Send + Sync {
    /// Dumps emails using the given unit of work.
    async fn dump_emails(&self, uow: &UoW) -> Result<Vec<String>, String>;
}
