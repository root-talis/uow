mod send_email;
mod dump_emails;

pub use send_email::{SendEmailRequest, SendEmailUseCase};
pub use dump_emails::DumpEmailsUseCase;

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use chrono::{DateTime, Utc};

    use super::{DumpEmailsUseCase, SendEmailRequest, SendEmailUseCase};

    use crate::application::dto::Message;
    use crate::application::ports::clock::Clock;
    use crate::application::ports::email::EmailTransactionalOutboxWriter;
    use crate::infrastructure::in_memory::email_repository::InMemoryEmailRepository;
    use crate::infrastructure::in_memory::platform::InMemoryPlatform;
    use crate::infrastructure::in_memory::store::Store;
    use crate::infrastructure::in_memory::transaction::{InMemoryUnitOfWork, InMemoryUnitOfWorkFactory};

    /// Test double clock that returns a fixed time.
    struct FixedClock {
        now: DateTime<Utc>,
    }

    impl FixedClock {
        fn new(now: DateTime<Utc>) -> Self {
            Self { now }
        }
    }

    impl Clock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            self.now
        }
    }

    /// In-memory email writer that always fails (for testing rollback).
    pub struct FailingEmailWriter;

    impl EmailTransactionalOutboxWriter<InMemoryPlatform, InMemoryUnitOfWork> for FailingEmailWriter {
        async fn send_email(&self, uow: &mut InMemoryUnitOfWork, msg: &Message) -> Result<(), String> {
            uow.tx_mut().add_email(msg); // pollute transaction to test that rollback actually happens
            Err("Email delivery failed".to_string())
        }
    }

    #[tokio::test]
    async fn test_use_case_commits_emails_on_success() {
        let store = Arc::new(Store::new());
        let factory = Arc::new(InMemoryUnitOfWorkFactory::new(store.clone()));
        let email_repo = Arc::new(InMemoryEmailRepository);
        let fixed_time = Utc::now();
        let clock = Arc::new(FixedClock::new(fixed_time));
        let send_usecase = SendEmailUseCase::new(factory, email_repo, clock);

        let result = send_usecase.execute(SendEmailRequest {
            message: "Hello, World!".to_string(),
        }).await;

        assert!(result.is_ok());
        let emails = store.get_emails();
        assert_eq!(emails.len(), 1);
        assert_eq!(emails[0].message, "Hello, World!");
        assert_eq!(emails[0].created_at, fixed_time);
    }

    #[tokio::test]
    async fn test_use_case_rolls_back_emails_on_failure() {
        let store = Arc::new(Store::new());
        let factory = Arc::new(InMemoryUnitOfWorkFactory::new(store.clone()));
        let email_writer = Arc::new(FailingEmailWriter);
        let fixed_time = Utc::now();
        let clock = Arc::new(FixedClock::new(fixed_time));
        let usecase = SendEmailUseCase::new(factory, email_writer, clock);

        let result = usecase.execute(SendEmailRequest {
            message: "This will fail".to_string(),
        }).await;

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
        let fixed_time = Utc::now();
        let clock = Arc::new(FixedClock::new(fixed_time));
        let usecase = SendEmailUseCase::new(factory, email_repo, clock);

        usecase.execute(SendEmailRequest {
            message: "First email".to_string(),
        }).await.unwrap();
        usecase.execute(SendEmailRequest {
            message: "Second email".to_string(),
        }).await.unwrap();

        let emails = store.get_emails();
        assert_eq!(emails.len(), 2);
        assert_eq!(emails[0].message, "First email");
        assert_eq!(emails[0].created_at, fixed_time);
        assert_eq!(emails[1].message, "Second email");
        assert_eq!(emails[1].created_at, fixed_time);
    }

    #[tokio::test]
    async fn test_send_and_dump_share_same_store() {
        let store = Arc::new(Store::new());
        let factory = Arc::new(InMemoryUnitOfWorkFactory::new(store.clone()));
        let email_repo = Arc::new(InMemoryEmailRepository);

        let fixed_time = Utc::now();
        let clock = Arc::new(FixedClock::new(fixed_time));
        let send_usecase = SendEmailUseCase::new(factory.clone(), email_repo.clone(), clock);
        let dump_usecase = DumpEmailsUseCase::new(factory.clone(), email_repo.clone());

        // Initially no emails
        let emails = dump_usecase.execute().await.unwrap();
        assert!(emails.is_empty());

        // Send an email
        send_usecase.execute(SendEmailRequest {
            message: "Shared email".to_string(),
        }).await.unwrap();

        // Dump should now see the committed email
        let emails = dump_usecase.execute().await.unwrap();
        assert_eq!(emails.len(), 1);
        assert_eq!(emails[0].message, "Shared email");
        assert_eq!(emails[0].created_at, fixed_time);
    }
}
