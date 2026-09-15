use thiserror::Error;
use tracing::error;
use uow::{TxManager, UoW, transaction};

use crate::{
    application::{errors::StorageError, ports::AccountsRepository},
    domain::value_objects::{AccountNumber, CreditsAmount},
};

#[derive(Clone)]
pub struct GetBalanceUsecase<M, A>
where
    M: TxManager,
    A: AccountsRepository<M::UoW>,
{
    tx_manager: M,
    accounts_repo: A,
}

impl<M, A> GetBalanceUsecase<M, A>
where
    M: TxManager,
    A: AccountsRepository<M::UoW>,
{
    pub fn new(tx_manager: M, accounts_repo: A) -> Self {
        Self {
            tx_manager,
            accounts_repo,
        }
    }

    pub async fn execute(
        &self,
        account_number: AccountNumber,
    ) -> Result<CreditsAmount, GetBalanceError<M::UoW>> {
        transaction!(self.tx_manager, |mut uow| async {
            let account = self
                .accounts_repo
                .get_account(&mut uow, account_number)
                .await?
                .ok_or(GetBalanceError::AccountNotAvailable(account_number))?;

            Ok(*account.balance())
        })
        .await
        .map_err(|e| {
            error!(e = e.to_string(), "transaction error");
            GetBalanceError::StorageError(StorageError::PlatformError(e.into_inner()))
        })?
    }
}

#[derive(Error, Debug)]
pub enum GetBalanceError<U: UoW> {
    #[error("account not available: {_0}")]
    AccountNotAvailable(AccountNumber),

    #[error("internal error")]
    StorageError(
        #[from]
        #[source]
        StorageError<U>,
    ),
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use uow::test_utils::{TestTxManager, TestUoWError};

    use super::{GetBalanceError, GetBalanceUsecase};
    use crate::application::errors::StorageError;
    use crate::application::usecases::tests::MockAccountsRepository;
    use crate::domain::{
        entities::Account,
        value_objects::{AccountNumber, CreditsAmount},
    };

    #[tokio::test]
    async fn execute_should_return_balance_when_account_exists() {
        let account_number = AccountNumber::new(NonZeroU32::new(42).unwrap());
        let expected_balance = CreditsAmount::new(1_000);
        let account = Account::new(account_number, expected_balance);

        let repo = MockAccountsRepository::new().with_get_account(Some(account));
        let usecase = GetBalanceUsecase::new(TestTxManager::new(), repo);

        let result = usecase.execute(account_number).await;

        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        assert_eq!(*result.unwrap().inner(), 1_000);
    }

    #[tokio::test]
    async fn execute_should_call_repository_with_account_number() {
        let account_number = AccountNumber::new(NonZeroU32::new(42).unwrap());
        let expected_balance = CreditsAmount::new(1_000);
        let account = Account::new(account_number, expected_balance);

        let repo = MockAccountsRepository::new().with_get_account(Some(account));
        let usecase = GetBalanceUsecase::new(TestTxManager::new(), repo.clone());

        let _ = usecase.execute(account_number).await;

        let calls = repo.get_account_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(*calls[0].inner(), NonZeroU32::new(42).unwrap());
    }

    #[tokio::test]
    async fn execute_should_return_account_not_available_error_when_account_missing() {
        let account_number = AccountNumber::new(NonZeroU32::new(99).unwrap());

        let repo = MockAccountsRepository::new().with_get_account(None);
        let usecase = GetBalanceUsecase::new(TestTxManager::new(), repo.clone());

        let result = usecase.execute(account_number).await;

        assert!(
            matches!(
                result,
                Err(GetBalanceError::AccountNotAvailable(ref an)) if *an.inner() == NonZeroU32::new(99).unwrap()
            ),
            "expected AccountNotAvailable, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn execute_should_return_storage_error_when_repository_fails() {
        let account_number = AccountNumber::new(NonZeroU32::new(7).unwrap());
        let storage_error = StorageError::PlatformError(TestUoWError);

        let repo = MockAccountsRepository::new().with_get_account_return(Err(storage_error));
        let usecase = GetBalanceUsecase::new(TestTxManager::new(), repo.clone());

        let result = usecase.execute(account_number).await;

        assert!(
            matches!(
                result,
                Err(GetBalanceError::StorageError(StorageError::PlatformError(_)))
            ),
            "expected StorageError::PlatformError, got {:?}",
            result
        );
    }
}
