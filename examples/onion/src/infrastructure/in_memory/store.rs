use std::sync::{Arc, Mutex};

use super::transaction::InMemoryTransaction;

/// In-memory store that acts like a database connection.
/// Holds the list of emails that have been committed.
pub struct Store {
    emails: Mutex<Vec<String>>,
}

impl Store {
    /// Creates a new empty store.
    pub fn new() -> Self {
        Self {
            emails: Mutex::new(Vec::new()),
        }
    }

    /// Begins a new transaction backed by this store.
    pub fn begin_transaction(self: &Arc<Self>) -> InMemoryTransaction {
        InMemoryTransaction::new(Arc::clone(self))
    }

    /// Adds emails to the store.
    pub(crate) fn add_emails(&self, emails: Vec<String>) {
        self.emails.lock().unwrap().extend(emails);
    }

    /// Returns a copy of all emails in the store.
    pub fn get_emails(&self) -> Vec<String> {
        self.emails.lock().unwrap().clone()
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}
