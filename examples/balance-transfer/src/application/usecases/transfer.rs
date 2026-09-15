use thiserror::Error;
use tracing::error;
use uow::{
    IsloationLevelNotSupportedError, IsolationLevel, IsolationLevelSelector, TxManager, UoW,
    transaction,
};

use crate::{
    application::{errors::StorageError, ports::AccountsRepository},
    domain::{
        self,
        services::transfer_credits,
        value_objects::{AccountNumber, CreditsAmount},
    },
};

pub struct TransferRequest {
    pub from: AccountNumber,
    pub to: AccountNumber,
    pub amount: CreditsAmount,
}

pub type TransferResult = ();

#[derive(Error, Debug)]
pub enum TransferError<U: UoW> {
    #[error("account not available: {_0}")]
    AccountNotAvailable(AccountNumber),

    #[error("failed to transfer credits: {_0}")]
    TransferFailed(
        #[from]
        #[source]
        domain::errors::TransferFailure,
    ),

    #[error("internal error")]
    IsolationLevelNotSupported(#[from] IsloationLevelNotSupportedError),

    #[error("internal error")]
    StorageError(
        #[from]
        #[source]
        StorageError<U>,
    ),
}

#[derive(Clone)]
pub struct TransferUsecase<M, A>
where
    M: TxManager,
    A: AccountsRepository<M::UoW>,
{
    tx_manager: M,
    accounts_repo: A,
}

impl<M, A> TransferUsecase<M, A>
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
        request: TransferRequest,
    ) -> Result<TransferResult, TransferError<M::UoW>> {
        transaction!(
            self.tx_manager
                .isolated_at(IsolationLevel::RepeatableRead)
                .or(IsolationLevel::Serializable)
                .if_platform_supports()?,
            |mut uow| async {
                let mut from_account = self
                    .accounts_repo
                    .get_account(&mut uow, request.from)
                    .await?
                    .ok_or(TransferError::AccountNotAvailable(request.from))?;

                let mut to_account = self
                    .accounts_repo
                    .get_account(&mut uow, request.to)
                    .await?
                    .ok_or(TransferError::AccountNotAvailable(request.to))?;

                transfer_credits(&mut from_account, request.amount, &mut to_account)?;

                self.accounts_repo
                    .save_account(&mut uow, &from_account)
                    .await?;
                self.accounts_repo
                    .save_account(&mut uow, &to_account)
                    .await?;

                Ok(())
            }
        )
        .await
        .map_err(|e| {
            error!(e = e.to_string(), "transaction error");
            TransferError::StorageError(StorageError::PlatformError(e.into_inner()))
        })?
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use uow::test_utils::{TestTxManager, TestUoWError};

    use super::{TransferError, TransferRequest, TransferUsecase};
    use crate::application::errors::StorageError;
    use crate::application::usecases::tests::MockAccountsRepository;
    use crate::domain::{
        entities::Account,
        errors::{TransferFailure, WithdrawFailure},
        value_objects::{AccountNumber, CreditsAmount},
    };

    fn account_number(n: u32) -> AccountNumber {
        AccountNumber::new(NonZeroU32::new(n).unwrap())
    }

    fn credits(a: u32) -> CreditsAmount {
        CreditsAmount::new(a)
    }

    #[tokio::test]
    async fn execute_should_return_ok_when_transfer_succeeds() {
        let from = account_number(1);
        let to = account_number(2);
        let amount = credits(50);

        let from_account = Account::new(from, credits(100));
        let to_account = Account::new(to, credits(20));

        let repo = MockAccountsRepository::new()
            .with_get_account(Some(from_account))
            .with_get_account(Some(to_account))
            .with_save_account(Ok(()))
            .with_save_account(Ok(()));

        let usecase = TransferUsecase::new(TestTxManager::new(), repo);
        let request = TransferRequest { from, to, amount };

        let result = usecase.execute(request).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn execute_should_call_get_account_for_both_accounts() {
        let from = account_number(1);
        let to = account_number(2);
        let amount = credits(50);

        let from_account = Account::new(from, credits(100));
        let to_account = Account::new(to, credits(20));

        let repo = MockAccountsRepository::new()
            .with_get_account(Some(from_account))
            .with_get_account(Some(to_account))
            .with_save_account(Ok(()))
            .with_save_account(Ok(()));

        let usecase = TransferUsecase::new(TestTxManager::new(), repo.clone());
        let request = TransferRequest { from, to, amount };

        let _result = usecase.execute(request).await;

        assert_eq!(repo.get_account_calls(), vec![from, to]);
    }

    #[tokio::test]
    async fn execute_should_call_save_account_for_both_accounts() {
        let from = account_number(1);
        let to = account_number(2);
        let amount = credits(50);

        let from_account = Account::new(from, credits(100));
        let to_account = Account::new(to, credits(20));

        let repo = MockAccountsRepository::new()
            .with_get_account(Some(from_account))
            .with_get_account(Some(to_account))
            .with_save_account(Ok(()))
            .with_save_account(Ok(()));

        let usecase = TransferUsecase::new(TestTxManager::new(), repo.clone());
        let request = TransferRequest { from, to, amount };

        let _result = usecase.execute(request).await;

        assert_eq!(repo.save_account_calls(), vec![from, to]);
    }

    #[tokio::test]
    async fn execute_should_return_account_not_available_error_when_from_account_missing() {
        let from = account_number(1);
        let to = account_number(2);
        let amount = credits(10);

        let repo = MockAccountsRepository::new().with_get_account(None);
        let usecase = TransferUsecase::new(TestTxManager::new(), repo);
        let request = TransferRequest { from, to, amount };

        let result = usecase.execute(request).await;

        assert!(matches!(
            result,
            Err(TransferError::AccountNotAvailable(n)) if n == from
        ));
    }

    #[tokio::test]
    async fn execute_should_return_account_not_available_error_when_to_account_missing() {
        let from = account_number(1);
        let to = account_number(2);
        let amount = credits(10);

        let from_account = Account::new(from, credits(100));

        let repo = MockAccountsRepository::new()
            .with_get_account(Some(from_account))
            .with_get_account(None);
        let usecase = TransferUsecase::new(TestTxManager::new(), repo);
        let request = TransferRequest { from, to, amount };

        let result = usecase.execute(request).await;

        assert!(matches!(
            result,
            Err(TransferError::AccountNotAvailable(n)) if n == to
        ));
    }

    #[tokio::test]
    async fn execute_should_return_transfer_failed_error_when_insufficient_funds() {
        let from = account_number(1);
        let to = account_number(2);
        let amount = credits(100);

        let from_account = Account::new(from, credits(50));
        let to_account = Account::new(to, credits(20));

        let repo = MockAccountsRepository::new()
            .with_get_account(Some(from_account))
            .with_get_account(Some(to_account));
        let usecase = TransferUsecase::new(TestTxManager::new(), repo);
        let request = TransferRequest { from, to, amount };

        let result = usecase.execute(request).await;

        assert!(matches!(
            result,
            Err(TransferError::TransferFailed(TransferFailure::WithdrawFailure(
                WithdrawFailure::InsufficientFunds
            )))
        ));
    }

    #[tokio::test]
    async fn execute_should_return_storage_error_when_get_account_fails() {
        let from = account_number(1);
        let to = account_number(2);
        let amount = credits(10);

        let repo = MockAccountsRepository::new()
            .with_get_account_return(Err(StorageError::PlatformError(TestUoWError)));
        let usecase = TransferUsecase::new(TestTxManager::new(), repo);
        let request = TransferRequest { from, to, amount };

        let result = usecase.execute(request).await;

        assert!(matches!(
            result,
            Err(super::TransferError::StorageError(StorageError::PlatformError(_)))
        ));
    }

    #[tokio::test]
    async fn execute_should_trigger_rollback_when_save_from_account_fails() {
        let from = account_number(1);
        let to = account_number(2);
        let amount = credits(10);

        let from_account = Account::new(from, credits(100));
        let to_account = Account::new(to, credits(20));

        let repo = MockAccountsRepository::new()
            .with_get_account(Some(from_account))
            .with_get_account(Some(to_account))
            .with_save_account(Err(StorageError::PlatformError(TestUoWError)));
        let spy = TestTxManager::new();
        let usecase = TransferUsecase::new(spy.clone(), repo.clone());
        let request = TransferRequest { from, to, amount };

        let result = usecase.execute(request).await;

        assert!(matches!(
            result,
            Err(TransferError::StorageError(StorageError::PlatformError(_)))
        ));
        assert_eq!(spy.transactions_rolled_back(), 1);
        assert_eq!(spy.transactions_committed(), 0);
        assert_eq!(repo.save_account_calls(), vec![from]);
    }

    #[tokio::test]
    async fn execute_should_trigger_rollback_when_save_to_account_fails() {
        let from = account_number(1);
        let to = account_number(2);
        let amount = credits(10);

        let from_account = Account::new(from, credits(100));
        let to_account = Account::new(to, credits(20));

        let repo = MockAccountsRepository::new()
            .with_get_account(Some(from_account))
            .with_get_account(Some(to_account))
            .with_save_account(Ok(()))
            .with_save_account(Err(StorageError::PlatformError(TestUoWError)));
        let spy = TestTxManager::new();
        let usecase = TransferUsecase::new(spy.clone(), repo.clone());
        let request = TransferRequest { from, to, amount };

        let result = usecase.execute(request).await;

        assert!(matches!(
            result,
            Err(TransferError::StorageError(StorageError::PlatformError(_)))
        ));
        assert_eq!(spy.transactions_rolled_back(), 1);
        assert_eq!(spy.transactions_committed(), 0);
        assert_eq!(repo.save_account_calls(), vec![from, to]);
    }

    #[tokio::test]
    async fn execute_should_trigger_commit_when_transfer_succeeds() {
        let from = account_number(1);
        let to = account_number(2);
        let amount = credits(50);

        let from_account = Account::new(from, credits(100));
        let to_account = Account::new(to, credits(20));

        let repo = MockAccountsRepository::new()
            .with_get_account(Some(from_account))
            .with_get_account(Some(to_account))
            .with_save_account(Ok(()))
            .with_save_account(Ok(()));
        let spy = TestTxManager::new();
        let usecase = TransferUsecase::new(spy.clone(), repo);
        let request = TransferRequest { from, to, amount };

        let result = usecase.execute(request).await;

        assert!(result.is_ok());
        assert_eq!(spy.transactions_committed(), 1);
        assert_eq!(spy.transactions_rolled_back(), 0);
    }
}
