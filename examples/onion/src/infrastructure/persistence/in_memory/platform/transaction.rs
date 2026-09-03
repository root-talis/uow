use std::sync::Arc;

use uow::{
    TxResult, UnitOfWork, UnitOfWorkFactory,
};
use crate::application::dto::Message;
use super::InMemoryPlatform;
use super::store::Store;

/// In-memory transaction that tracks sent emails.
/// Created by a Store and can commit its changes back to that store.
pub struct InMemoryTransaction {
    store: Arc<Store>,
    emails: Vec<Message>,
}

impl InMemoryTransaction {
    /// Creates a new transaction backed by the given store.
    pub(crate) fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            emails: Vec::new(),
        }
    }

    /// Adds an email to this transaction.
    pub fn add_email(&mut self, msg: &Message) {
        self.emails.push(Message {
            message: msg.message.clone(),
            created_at: msg.created_at,
        });
    }

    // Reads emails from the store and this transaction
    pub fn read_emails(&self) -> Vec<Message> {
        [self.store.get_emails().as_slice(), self.emails.as_slice()].concat()
    }
}

/// Trait that allows committing or rolling back a transaction.
/// This trait is private to the transaction module so that only
/// InMemoryUnitOfWorkFactory (which lives in this module) can call these methods.
pub(crate) trait Commit {
    fn commit(self);
    fn rollback(&mut self);
}

impl Commit for InMemoryTransaction {
    fn commit(self) {
        self.store.add_emails(self.emails);
    }

    fn rollback(&mut self) {
        self.emails.clear();
    }
}

/// In-memory unit of work that holds a transaction.
pub struct InMemoryUnitOfWork {
    tx: InMemoryTransaction,
}

impl InMemoryUnitOfWork {
    /// Creates a new in-memory unit of work with the given transaction.
    pub fn new(tx: InMemoryTransaction) -> Self {
        Self { tx }
    }

    /// Returns a mutable reference to the transaction.
    pub fn tx_mut(&mut self) -> &mut InMemoryTransaction {
        &mut self.tx
    }

    /// Returns an immutable reference to the transaction.
    pub fn tx(&self) -> &InMemoryTransaction {
        &self.tx
    }
}

impl UnitOfWork<InMemoryPlatform> for InMemoryUnitOfWork {}

/// In-memory unit of work factory.
/// Holds a reference to a Store, acting like a database connection.
pub struct InMemoryUnitOfWorkFactory {
    store: Arc<Store>,
}

impl InMemoryUnitOfWorkFactory {
    /// Creates a new factory with the given store.
    pub fn new(store: Arc<Store>) -> Self {
        Self { store }
    }
}

impl UnitOfWorkFactory<InMemoryPlatform> for InMemoryUnitOfWorkFactory {
    type UoW<'a> = InMemoryUnitOfWork;

    async fn with_unitofwork<T, E, F>(&self, f: F) -> Result<T, E>
    where
        T: Send,
        E: Send,
        F: for <'a> AsyncFnOnce(&mut Self::UoW<'a>) -> Result<TxResult<T>, TxResult<E>> + Send,
    {
        let mut uow = InMemoryUnitOfWork::new(self.store.begin_transaction());
        let result = f(&mut uow).await;

        match result {
            Ok(TxResult::Commit(t)) => {
                // Commit: dump emails into the store.
                // Commit::commit(uow.tx);
                uow.tx.commit();
                Ok(t)
            }
            Ok(TxResult::Rollback(t)) => {
                // Rollback: discard emails.
                // Commit::rollback(&mut uow.tx);
                uow.tx.rollback();
                Ok(t)
            }
            Err(TxResult::Commit(e)) => {
                // Commit on error: dump emails into the store.
                // Commit::commit(uow.tx);
                uow.tx.commit();
                Err(e)
            }
            Err(TxResult::Rollback(e)) => {
                // Rollback on error: discard emails.
                // Commit::rollback(&mut uow.tx);
                uow.tx.rollback();
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::application::dto::Message;
    use crate::infrastructure::persistence::in_memory::platform::{store::Store, transaction::InMemoryUnitOfWorkFactory};
    use uow::{TxResult, UnitOfWorkFactory};

    #[tokio::test]
    async fn test_with_unitofwork_commit_ok_dumps_emails() {
        let store = Arc::new(Store::new());
        let factory = InMemoryUnitOfWorkFactory::new(store.clone());
        let fixed_time = chrono::Utc::now();
        let result: Result<&str, &str> = factory
            .with_unitofwork(async |uow| {
                uow.tx_mut().add_email(&Message {
                    message: "test@example.com".to_string(),
                    created_at: fixed_time,
                });
                Ok(TxResult::commit("success"))
            })
            .await;

        assert_eq!(result.unwrap(), "success");
        let emails = store.get_emails();
        assert_eq!(emails.len(), 1);
        assert_eq!(emails[0].message, "test@example.com");
    }

    #[tokio::test]
    async fn test_with_unitofwork_rollback_ok_discards_emails() {
        let store = Arc::new(Store::new());
        let factory = InMemoryUnitOfWorkFactory::new(store.clone());
        let fixed_time = chrono::Utc::now();
        let result: Result<&str, &str> = factory
            .with_unitofwork(async |uow| {
                uow.tx_mut().add_email(&Message {
                    message: "test@example.com".to_string(),
                    created_at: fixed_time,
                });
                Ok(TxResult::rollback("success"))
            })
            .await;

        assert_eq!(result.unwrap(), "success");
        let emails = store.get_emails();
        assert!(emails.is_empty());
    }

    #[tokio::test]
    async fn test_with_unitofwork_commit_err_dumps_emails() {
        let store = Arc::new(Store::new());
        let factory = InMemoryUnitOfWorkFactory::new(store.clone());
        let fixed_time = chrono::Utc::now();
        let result: Result<&str, &str> = factory
            .with_unitofwork(async |uow| {
                uow.tx_mut().add_email(&Message {
                    message: "test@example.com".to_string(),
                    created_at: fixed_time,
                });
                Err(TxResult::commit("error"))
            })
            .await;

        assert_eq!(result.unwrap_err(), "error");
        let emails = store.get_emails();
        assert_eq!(emails.len(), 1);
        assert_eq!(emails[0].message, "test@example.com");
    }

    #[tokio::test]
    async fn test_with_unitofwork_rollback_err_discards_emails() {
        let store = Arc::new(Store::new());
        let factory = InMemoryUnitOfWorkFactory::new(store.clone());
        let fixed_time = chrono::Utc::now();
        let result: Result<&str, &str> = factory
            .with_unitofwork(async |uow| {
                uow.tx_mut().add_email(&Message {
                    message: "test@example.com".to_string(),
                    created_at: fixed_time,
                });
                Err(TxResult::rollback("error"))
            })
            .await;

        assert_eq!(result.unwrap_err(), "error");
        let emails = store.get_emails();
        assert!(emails.is_empty());
    }
}
