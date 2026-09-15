use std::num::{NonZeroI64, NonZeroU32, TryFromIntError};

use async_trait::async_trait;
use axum::extract::FromRef;

use crate::{application::{errors::StorageError, ports::AccountsRepository}, domain::{entities::Account, value_objects::{AccountNumber, CreditsAmount}}};
use uow::sqlx::sqlite::SqliteUoW;

#[derive(Clone)]
pub struct SqliteRepository;


#[async_trait]
impl AccountsRepository<SqliteUoW> for SqliteRepository {
    async fn get_account(&self, uow: &mut SqliteUoW, account_number: AccountNumber) -> Result<Option<Account>, StorageError<SqliteUoW>> {
        let acc = sqlx::query_as!(AccountData, "SELECT id, balance FROM accounts WHERE id = $1", account_number.inner())
            .fetch_optional(&mut **uow.tx_mut())
            .await
            .map_err(StorageError::PlatformError)?;

        let Some(acc) = acc else {
            return Ok(None);
        };

        let acc: Account = acc.try_into().map_err(|e| StorageError::StorageFormatError(Box::new(e)))?;

        Ok(Some(acc))
    }

    async fn save_account(&self, uow: &mut SqliteUoW, account: &Account) -> Result<(), StorageError<SqliteUoW>> {
        let account_data: AccountData = FromRef::from_ref(account);

        sqlx::query!("UPDATE accounts SET balance = $1 WHERE id = $2", account_data.balance, account_data.id)
            .execute(&mut **uow.tx_mut())
            .await
            .map_err(StorageError::PlatformError)?;
        
        Ok(())
    }
}

/// Temporary struct to read account data from Sqlite db
struct AccountData {
    id:      i64,
    balance: i64,
}

/// Convert sqlite account table data into Account entity
impl TryInto<Account> for AccountData {
    type Error = TryFromIntError;

    fn try_into(self) -> Result<Account, Self::Error> {
        Ok(Account::new(
            self.id.try_into()?,
            self.balance.try_into()?
        ))
    }
}

impl FromRef<Account> for AccountData {
    fn from_ref(value: &Account) -> Self {
        Self {
            id:      FromRef::from_ref(value.number()),
            balance: FromRef::from_ref(value.balance()),
        }
    }
}


/// Read AccountNumber from the way it is stored in Sqlite
impl TryInto<AccountNumber> for i64 {
    type Error = TryFromIntError;

    fn try_into(self) -> Result<AccountNumber, Self::Error> {
        let accid: NonZeroI64 = self.try_into()?;
        let accid: NonZeroU32 = accid.try_into()?;
        Ok(AccountNumber::new(accid))
    }
}

impl FromRef<AccountNumber> for i64 {
    fn from_ref(value: &AccountNumber) -> Self {
        let u32: u32 = (*value.inner()).into();
        u32.into()
    }
}


/// Read CreditsAmount from the way it is stored in Sqlite
impl TryInto<CreditsAmount> for i64 {
    type Error = TryFromIntError;

    fn try_into(self) -> Result<CreditsAmount, Self::Error> {
        Ok(CreditsAmount::new(self.try_into()?))
    }
}

impl FromRef<CreditsAmount> for i64 {
    fn from_ref(value: &CreditsAmount) -> Self {
       (*value.inner()).into()
    }
}
