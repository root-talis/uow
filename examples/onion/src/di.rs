use std::sync::Arc;

use crate::application::usecase::{DumpEmailsUseCase, SendEmailUseCase};
use crate::infrastructure::clock::SystemClock;
use crate::infrastructure::persistence::in_memory::repository::email_repository::InMemoryEmailRepository;
use crate::infrastructure::persistence::in_memory::platform::InMemoryPlatform;
use crate::infrastructure::persistence::in_memory::platform::store::Store;
use crate::infrastructure::persistence::in_memory::platform::transaction::InMemoryUnitOfWorkFactory;

/// Dependency injection container holding concrete use cases.
#[derive(Clone)]
pub struct Container {
    pub send_email: SendEmailUseCase<
        InMemoryPlatform,
        InMemoryUnitOfWorkFactory,
        InMemoryEmailRepository,
    >,
    pub dump_emails: DumpEmailsUseCase<
        InMemoryPlatform,
        InMemoryUnitOfWorkFactory,
        InMemoryEmailRepository,
    >,
}

impl Container {
    /// Bootstraps the container with in-memory infrastructure.
    pub fn new() -> Self {
        let store = Arc::new(Store::new());
        let factory = Arc::new(InMemoryUnitOfWorkFactory::new(store));
        let email_repo = Arc::new(InMemoryEmailRepository);
        let clock = Arc::new(SystemClock);

        let send_email = SendEmailUseCase::new(factory.clone(), email_repo.clone(), clock);
        let dump_emails = DumpEmailsUseCase::new(factory, email_repo);

        Self {
            send_email,
            dump_emails,
        }
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}
