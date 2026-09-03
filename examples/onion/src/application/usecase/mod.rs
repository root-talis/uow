mod send_email;
mod dump_emails;

pub use send_email::SendEmailUseCase;
pub use dump_emails::DumpEmailsUseCase;

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use super::{DumpEmailsUseCase, SendEmailUseCase};

    use crate::application::ports::email::EmailTransactionalOutboxWriter;
    use crate::infrastructure::in_memory::email_repository::InMemoryEmailRepository;
    use crate::infrastructure::in_memory::platform::InMemoryPlatform;
    use crate::infrastructure::in_memory::store::Store;
    use crate::infrastructure::in_memory::transaction::{InMemoryUnitOfWork, InMemoryUnitOfWorkFactory};

    /// In-memory email writer that always fails (for testing rollback).
    pub struct FailingEmailWriter;

    impl EmailTransactionalOutboxWriter<InMemoryPlatform, InMemoryUnitOfWork> for FailingEmailWriter {
        async fn send_email(&self, uow: &mut InMemoryUnitOfWork, msg: &str) -> Result<(), String> {
            uow.tx_mut().add_email(msg); // pollute transaction to test that rollback actually happens
            Err("Email delivery failed".to_string())
        }
    }

    #[tokio::test]
    async fn test_use_case_commits_emails_on_success() {
        let store = Arc::new(Store::new());
        let factory = Arc::new(InMemoryUnitOfWorkFactory::new(store.clone()));
        let email_repo = Arc::new(InMemoryEmailRepository);
        let send_usecase = SendEmailUseCase::new(factory, email_repo);

        let result = send_usecase.execute("Hello, World!").await;

        assert!(result.is_ok());
        let emails = store.get_emails();
        assert_eq!(emails.len(), 1);
        assert_eq!(emails[0], "Hello, World!");
    }

    #[tokio::test]
    async fn test_use_case_rolls_back_emails_on_failure() {
        let store = Arc::new(Store::new());
        let factory = Arc::new(InMemoryUnitOfWorkFactory::new(store.clone()));
        let email_writer = Arc::new(FailingEmailWriter);
        let usecase = SendEmailUseCase::new(factory, email_writer);

        let result = usecase.execute("This will fail").await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Email delivery failed");
        let emails = store.get_emails();
        assert!(emails.is_empty(), "Emails should be discarded on rollback");
    }

    #[tokio::test]
    async fn test_multiple_emails_committed() {
        let store = Arc::new(Store::new());
        let factory = Arc::new(InMemoryUnitOfWorkFactory::new(store.clone()));
        let email_repo = Arc::new(InMemoryEmailRepository);
        let usecase = SendEmailUseCase::new(factory, email_repo);

        usecase.execute("First email").await.unwrap();
        usecase.execute("Second email").await.unwrap();

        let emails = store.get_emails();
        assert_eq!(emails.len(), 2);
        assert_eq!(emails[0], "First email");
        assert_eq!(emails[1], "Second email");
    }

    #[tokio::test]
    async fn test_send_and_dump_share_same_store() {
        let store = Arc::new(Store::new());
        let factory = Arc::new(InMemoryUnitOfWorkFactory::new(store.clone()));
        let email_repo = Arc::new(InMemoryEmailRepository);

        let send_usecase = SendEmailUseCase::new(factory.clone(), email_repo.clone());
        let dump_usecase = DumpEmailsUseCase::new(factory.clone(), email_repo.clone());

        // Initially no emails
        let emails = dump_usecase.execute().await.unwrap();
        assert!(emails.is_empty());

        // Send an email
        send_usecase.execute("Shared email").await.unwrap();

        // Dump should now see the committed email
        let emails = dump_usecase.execute().await.unwrap();
        assert_eq!(emails.len(), 1);
        assert_eq!(emails[0], "Shared email");
    }
}
