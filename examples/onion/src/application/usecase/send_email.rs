use std::marker::PhantomData;
use std::sync::Arc;

use crate::application::dto::Message;
use crate::application::ports::clock::Clock;
use crate::application::ports::email::EmailTransactionalOutboxWriter;
use uow::{DatabasePlatform, TxResult, UnitOfWorkFactory};

/// Request to send an email.
pub struct SendEmailRequest {
    pub message: String,
}

/// Use case example that guarantees all dependencies
/// work with the same database platform at compile time.
pub struct SendEmailUseCase<P, F, E>
where
    P: DatabasePlatform,
    F: UnitOfWorkFactory<P>,
    E: EmailTransactionalOutboxWriter<P, F::UoW>,
{
    uow_factory: Arc<F>,
    email_writer: Arc<E>,
    clock: Arc<dyn Clock>,
    _phantom: PhantomData<P>,
}

impl<P, F, E> SendEmailUseCase<P, F, E>
where
    P: DatabasePlatform,
    F: UnitOfWorkFactory<P>,
    E: EmailTransactionalOutboxWriter<P, F::UoW>,
{
    /// Creates a new use case with the given factory, email writer, and clock.
    /// The compiler guarantees that both are for the same database platform.
    pub fn new(uow_factory: Arc<F>, email_writer: Arc<E>, clock: Arc<dyn Clock>) -> Self {
        Self {
            uow_factory,
            email_writer,
            clock,
            _phantom: PhantomData,
        }
    }

    /// Executes the use case by opening a unit of work,
    /// sending an email, and returning the result.
    ///
    /// On success, the transaction is committed.
    /// On failure, the transaction is rolled back.
    pub async fn execute(&self, request: SendEmailRequest) -> Result<(), String> {
        let message = Message {
            message: request.message,
            created_at: self.clock.now(),
        };

        self.uow_factory
            .with_unitofwork(async |uow| {
                self.email_writer
                    .send_email(uow, &message)
                    .await
                    .map(|_| TxResult::commit(()))
                    .map_err(TxResult::rollback)
            })
            .await
    }
}
