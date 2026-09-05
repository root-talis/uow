use sqlx::SqlitePool;

use crate::application::{GetBalanceUsecase, TransferUsecase};
use crate::infrastructure::SqlxRepository;
use crate::uow_sqlx::SqlxTxManager;

#[derive(Clone)]
pub struct Container {
    pub transfer_usecase: TransferUsecase<SqlxTxManager, SqlxRepository, SqlxRepository>,
    pub get_balance_usecase: GetBalanceUsecase<SqlxTxManager, SqlxRepository>,
}

impl Container {
    pub fn new(pool: SqlitePool) -> Self {
        let tx_man = SqlxTxManager::new(pool);
        let repository = SqlxRepository;
        let transfer_usecase = TransferUsecase::new(tx_man.clone(), repository.clone(), repository.clone());
        let get_balance_usecase = GetBalanceUsecase::new(tx_man, repository);
        Self {
            transfer_usecase,
            get_balance_usecase
        }
    }
}
