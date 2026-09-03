use crate::application::dto::Message;
use crate::application::ports::email::{EmailTransactionalOutboxReader, EmailTransactionalOutboxWriter};
use crate::infrastructure::persistence::in_memory::platform::InMemoryPlatform;
use crate::infrastructure::persistence::in_memory::platform::transaction::InMemoryUnitOfWork;

/// In-memory email repository that works via the unit of work's transaction.
pub struct InMemoryEmailRepository;

impl EmailTransactionalOutboxWriter<InMemoryPlatform, InMemoryUnitOfWork> for InMemoryEmailRepository {
    async fn send_email(&self, uow: &mut InMemoryUnitOfWork, msg: &Message) -> Result<(), String> {
        uow.tx_mut().add_email(msg);
        Ok(())
    }
}

impl EmailTransactionalOutboxReader<InMemoryPlatform, InMemoryUnitOfWork> for InMemoryEmailRepository {
    async fn dump_emails(&self, uow: &InMemoryUnitOfWork) -> Result<Vec<Message>, String> {
        Ok(uow.tx().read_emails())
    }
}
