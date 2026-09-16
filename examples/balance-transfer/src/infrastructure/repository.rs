use async_trait::async_trait;
use axum::extract::FromRef;

use crate::{
    application::{errors::StorageError, ports::AccountsRepository},
    domain::{entities::Account, value_objects::AccountNumber},
};
use uow::sqlx::sqlite::SqliteUoW;

use super::db_models::AccountData;

#[derive(Clone)]
pub struct SqliteRepository;

#[async_trait]
impl AccountsRepository<SqliteUoW> for SqliteRepository {
    async fn get_account(
        &self,
        uow: &mut SqliteUoW,
        account_number: AccountNumber,
    ) -> Result<Option<Account>, StorageError<SqliteUoW>> {
        let acc = sqlx::query_as!(
            AccountData,
            "SELECT id, balance FROM accounts WHERE id = $1",
            account_number.inner()
        )
        .fetch_optional(&mut **uow.tx_mut())
        .await
        .map_err(StorageError::PlatformError)?;

        let Some(acc) = acc else {
            return Ok(None);
        };

        let acc: Account = acc
            .try_into()
            .map_err(|e| StorageError::StorageFormatError(Box::new(e)))?;

        Ok(Some(acc))
    }

    async fn save_account(
        &self,
        uow: &mut SqliteUoW,
        account: &Account,
    ) -> Result<(), StorageError<SqliteUoW>> {
        let account_data: AccountData = FromRef::from_ref(account);

        sqlx::query!(
            "INSERT INTO accounts (id, balance) VALUES ($1, $2)
             ON CONFLICT(id) DO UPDATE SET balance = excluded.balance",
            account_data.id,
            account_data.balance
        )
        .execute(&mut **uow.tx_mut())
        .await
        .map_err(StorageError::PlatformError)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use sqlx::sqlite::SqlitePoolOptions;
    use uow::sqlx::sqlite::SqliteTxManager;
    use uow::{TxManager, transaction};

    use super::*;
    use crate::domain::value_objects::CreditsAmount;

    async fn setup_db() -> (sqlx::SqlitePool, SqliteTxManager) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");
        let tx_manager = SqliteTxManager::new(pool.clone());
        (pool, tx_manager)
    }

    fn account_number(n: u32) -> AccountNumber {
        AccountNumber::new(NonZeroU32::new(n).unwrap())
    }

    fn credits_amount(a: u32) -> CreditsAmount {
        CreditsAmount::new(a)
    }

    mod accounts_repository {
        use super::*;

        mod get_account {
            use super::*;

            #[tokio::test]
            async fn returns_some_when_account_exists() {
                let (_pool, tx_manager) = setup_db().await;
                let repo = SqliteRepository;

                let result = transaction!(tx_manager, |mut uow| async {
                    repo.get_account(&mut uow, account_number(1)).await
                })
                .await;

                let account = result.unwrap().unwrap().unwrap();
                assert_eq!(*account.number().inner(), NonZeroU32::new(1).unwrap());
                assert_eq!(*account.balance().inner(), 100_000);
            }

            #[tokio::test]
            async fn returns_none_when_account_missing() {
                let (_pool, tx_manager) = setup_db().await;
                let repo = SqliteRepository;

                let result = transaction!(tx_manager, |mut uow| async {
                    repo.get_account(&mut uow, account_number(999)).await
                })
                .await;

                assert!(result.unwrap().unwrap().is_none());
            }

            #[tokio::test]
            async fn returns_storage_format_error_on_negative_balance() {
                let (pool, tx_manager) = setup_db().await;
                // Insert corrupted data directly
                sqlx::query("INSERT INTO accounts (id, balance) VALUES (99, -1)")
                    .execute(&pool)
                    .await
                    .unwrap();

                let repo = SqliteRepository;
                let result = transaction!(tx_manager, |mut uow| async {
                    repo.get_account(&mut uow, account_number(99)).await
                })
                .await;

                assert!(
                    matches!(
                        result.unwrap().unwrap_err(),
                        StorageError::StorageFormatError(_)
                    ),
                    "expected StorageFormatError for negative balance"
                );
            }
        }

        mod save_account {
            use super::*;

            #[tokio::test]
            async fn persists_balance_changes() {
                let (_pool, tx_manager) = setup_db().await;
                let repo = SqliteRepository;

                // Load account 1, change balance, save — all in one transaction
                transaction!(tx_manager, |mut uow| async {
                    let mut account = repo
                        .get_account(&mut uow, account_number(1))
                        .await
                        .unwrap()
                        .unwrap();
                    account.deposit(credits_amount(500));
                    repo.save_account(&mut uow, &account).await.unwrap();
                    Ok::<(), StorageError<SqliteUoW>>(())
                })
                .await
                .unwrap()
                .unwrap();

                // Read back in a fresh transaction to verify persistence
                let result = transaction!(tx_manager, |mut uow| async {
                    repo.get_account(&mut uow, account_number(1)).await
                })
                .await;

                let updated = result.unwrap().unwrap().unwrap();
                assert_eq!(*updated.balance().inner(), 100_500);
            }

            #[tokio::test]
            async fn roundtrip_with_get_account() {
                let (_pool, tx_manager) = setup_db().await;
                let repo = SqliteRepository;

                // Update account 2 to a known balance
                transaction!(tx_manager, |mut uow| async {
                    let account = Account::new(account_number(2), credits_amount(42));
                    repo.save_account(&mut uow, &account).await.unwrap();
                    Ok::<(), StorageError<SqliteUoW>>(())
                })
                .await
                .unwrap()
                .unwrap();

                // Read back and verify
                let result = transaction!(tx_manager, |mut uow| async {
                    repo.get_account(&mut uow, account_number(2)).await
                })
                .await;

                let retrieved = result.unwrap().unwrap().unwrap();
                assert_eq!(*retrieved.number().inner(), NonZeroU32::new(2).unwrap());
                assert_eq!(*retrieved.balance().inner(), 42);
            }
        }

        mod transactional_behaviour {
            use super::*;

            #[tokio::test]
            async fn committed_save_is_visible_to_subsequent_read() {
                let (_pool, tx_manager) = setup_db().await;
                let repo = SqliteRepository;

                // Save within a transaction that commits
                let updated_account = Account::new(account_number(1), credits_amount(777));
                let result = transaction!(tx_manager, |mut uow| async {
                    repo.save_account(&mut uow, &updated_account).await?;
                    Ok::<(), StorageError<SqliteUoW>>(())
                })
                .await;

                assert!(result.is_ok());
                assert!(result.unwrap().is_ok());

                // Verify with a fresh transaction
                let result = transaction!(tx_manager, |mut uow| async {
                    repo.get_account(&mut uow, account_number(1)).await
                })
                .await;

                let account = result.unwrap().unwrap().unwrap();
                assert_eq!(*account.balance().inner(), 777);
            }

            #[tokio::test]
            async fn rolled_back_save_is_not_visible() {
                let (_pool, tx_manager) = setup_db().await;
                let repo = SqliteRepository;

                // Attempt save within a transaction that rolls back
                let updated_account = Account::new(account_number(2), credits_amount(999));
                let result = transaction!(tx_manager, |mut uow| async {
                    repo.save_account(&mut uow, &updated_account).await?;
                    // Force a database error to trigger rollback
                    let _ = sqlx::query("SELECT * FROM nonexistent_table")
                        .fetch_one(&mut **uow.tx_mut())
                        .await
                        .map_err(StorageError::<SqliteUoW>::PlatformError)?;
                    Ok::<(), StorageError<SqliteUoW>>(())
                })
                .await;

                // The outer result is Ok because the transaction machinery worked,
                // but the inner result is Err because the closure returned an error
                assert!(result.is_ok());
                assert!(result.unwrap().is_err());

                // Verify the old value is still there
                let result = transaction!(tx_manager, |mut uow| async {
                    repo.get_account(&mut uow, account_number(2)).await
                })
                .await;

                let account = result.unwrap().unwrap().unwrap();
                assert_eq!(*account.balance().inner(), 200_000);
            }
        }
    }
}
