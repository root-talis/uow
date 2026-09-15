mod get_balance;
pub use get_balance::*;

mod transfer;
pub use transfer::*;

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use crate::{
        application::{errors::StorageError, ports::AccountsRepository},
        domain::{entities::Account, value_objects::AccountNumber},
    };
    use uow::test_utils::TestUoW;

    // ------------------------------------------------------------------
    // MockAccountsRepository
    // ------------------------------------------------------------------

    type GetAccountResult = Result<Option<Account>, StorageError<TestUoW>>;
    type SaveAccountResult = Result<(), StorageError<TestUoW>>;

    // We use Arc<Mutex<...>> (not RefCell) because AccountsRepository is Send + Sync
    // and async_trait futures must be Send. RefCell is !Sync, so it cannot be shared
    // across await points in async trait methods.
    #[derive(Debug, Clone)]
    pub struct MockAccountsRepository {
        get_account_return: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<GetAccountResult>>>,
        save_account_return: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<SaveAccountResult>>>,

        get_account_calls: std::sync::Arc<std::sync::Mutex<Vec<AccountNumber>>>,
        save_account_calls: std::sync::Arc<std::sync::Mutex<Vec<AccountNumber>>>,
    }

    impl MockAccountsRepository {
        pub fn new() -> Self {
            Self {
                get_account_return: std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
                save_account_return: std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
                get_account_calls: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                save_account_calls: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            }
        }

        pub fn with_get_account(self, account: Option<Account>) -> Self {
            self.get_account_return.lock().unwrap().push_back(Ok(account));
            self
        }

        pub fn with_get_account_return(self, result: GetAccountResult) -> Self {
            self.get_account_return.lock().unwrap().push_back(result);
            self
        }

        pub fn with_save_account(self, result: SaveAccountResult) -> Self {
            self.save_account_return.lock().unwrap().push_back(result);
            self
        }

        pub fn get_account_calls(&self) -> Vec<AccountNumber> {
            self.get_account_calls.lock().unwrap().clone()
        }

        pub fn save_account_calls(&self) -> Vec<AccountNumber> {
            self.save_account_calls.lock().unwrap().clone()
        }
    }

    impl Default for MockAccountsRepository {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl AccountsRepository<TestUoW> for MockAccountsRepository {
        async fn get_account(
            &self,
            _uow: &mut TestUoW,
            account_number: AccountNumber,
        ) -> Result<Option<Account>, StorageError<TestUoW>> {
            self.get_account_calls.lock().unwrap().push(account_number);
            self.get_account_return
                .lock()
                .unwrap()
                .pop_front()
                .expect("mock return not configured")
        }

        async fn save_account(
            &self,
            _uow: &mut TestUoW,
            account: &Account,
        ) -> SaveAccountResult {
            self.save_account_calls
                .lock()
                .unwrap()
                .push(account.number().clone());
            self.save_account_return
                .lock()
                .unwrap()
                .pop_front()
                .expect("mock return not configured")
        }
    }

    // ------------------------------------------------------------------
    // Smoke tests
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn mock_accounts_repository_smoke_test() {
        use std::num::NonZeroU32;

        let account_number = AccountNumber::new(NonZeroU32::new(123).unwrap());
        let account = Account::new(account_number, crate::domain::value_objects::CreditsAmount::new(100));

        let repo = MockAccountsRepository::new()
            .with_get_account(Some(account.clone()))
            .with_save_account(Ok(()));

        let mut uow = TestUoW;

        // Test get_account
        let result = repo.get_account(&mut uow, *account.number()).await;
        let returned = result.unwrap();
        assert!(returned.is_some());
        assert_eq!(returned.unwrap().number().inner(), account.number().inner());
        assert_eq!(repo.get_account_calls().len(), 1);

        // Test save_account
        let result = repo.save_account(&mut uow, &account).await;
        assert!(result.is_ok());
        assert_eq!(repo.save_account_calls().len(), 1);
    }
}
