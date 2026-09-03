use std::marker::PhantomData;
use std::sync::Arc;

use crate::application::dto::Message;
use crate::application::ports::email::EmailTransactionalOutboxReader;
use uow::{DatabasePlatform, TxResult, UnitOfWorkFactory};

pub struct DumpEmailsUseCase<P, F, E>
where
    P: DatabasePlatform,
    F: UnitOfWorkFactory<P>,
    E: EmailTransactionalOutboxReader<P, F::UoW>,
{
    uow_factory: Arc<F>,
    email_reader: Arc<E>,
    _phantom: PhantomData<P>,
}

impl<P, F, E> Clone for DumpEmailsUseCase<P, F, E>
where
    P: DatabasePlatform,
    F: UnitOfWorkFactory<P>,
    E: EmailTransactionalOutboxReader<P, F::UoW>,
{
    fn clone(&self) -> Self {
        Self {
            uow_factory: Arc::clone(&self.uow_factory),
            email_reader: Arc::clone(&self.email_reader),
            _phantom: PhantomData,
        }
    }
}

impl<P, F, E> DumpEmailsUseCase<P, F, E>
where
    P: DatabasePlatform,
    F: UnitOfWorkFactory<P>,
    E: EmailTransactionalOutboxReader<P, F::UoW>,
{
    /// Creates a new use case with the given factory and email reader.
    /// The compiler guarantees that both are for the same database platform.
    pub fn new(uow_factory: Arc<F>, email_reader: Arc<E>) -> Self {
        Self {
            uow_factory,
            email_reader,
            _phantom: PhantomData,
        }
    }

    /// Executes the use case by opening a unit of work,
    /// dumping emails, and returning the result.
    pub async fn execute(&self) -> Result<Vec<Message>, String> {
        self.uow_factory
            .with_unitofwork(async |uow| {
                self.email_reader
                    .dump_emails(uow)
                    .await
                    .map(|v| TxResult::rollback(v))
                    .map_err(TxResult::rollback)
            })
            .await
    }
}
