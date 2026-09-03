use crate::application::ports::email::{EmailTransactionalOutboxReader, EmailTransactionalOutboxWriter};
use super::platform::InMemoryPlatform;
use super::transaction::InMemoryUnitOfWork;

/// In-memory email repository that works via the unit of work's transaction.
pub struct InMemoryEmailRepository;

impl EmailTransactionalOutboxWriter<InMemoryPlatform, InMemoryUnitOfWork> for InMemoryEmailRepository {
    async fn send_email(&self, uow: &mut InMemoryUnitOfWork, msg: &str) -> Result<(), String> {
        uow.tx_mut().add_email(msg);
        Ok(())
    }
}

impl EmailTransactionalOutboxReader<InMemoryPlatform, InMemoryUnitOfWork> for InMemoryEmailRepository {
    async fn dump_emails(&self, uow: &InMemoryUnitOfWork) -> Result<Vec<String>, String> {
        Ok(uow.tx().read_emails())
    }
}
