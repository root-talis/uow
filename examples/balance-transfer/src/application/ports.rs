use async_trait::async_trait;

use uow::UoW;

use crate::{
    application::errors::StorageError,
    domain::{entities::Account, value_objects::AccountNumber},
};

#[async_trait]
pub trait AccountsRepository<U: UoW>: Clone + Send + Sync {
    async fn get_account(
        &self,
        uow: &mut U,
        account_number: AccountNumber,
    ) -> Result<Option<Account>, StorageError<U>>;

    async fn save_account(&self, uow: &mut U, account: &Account) -> Result<(), StorageError<U>>;
}
