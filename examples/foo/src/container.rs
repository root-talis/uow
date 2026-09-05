use sqlx::SqlitePool;

use crate::application::usecases::{GetBalanceUsecase, TransferUsecase};
use crate::infrastructure::SqliteRepository;
use uow::sqlx::sqlite::SqliteTxManager;

#[derive(Clone)]
pub struct Container {
    pub transfer_usecase: TransferUsecase<SqliteTxManager, SqliteRepository, SqliteRepository>,
    pub get_balance_usecase: GetBalanceUsecase<SqliteTxManager, SqliteRepository>,
}

impl Container {
    pub fn new(pool: SqlitePool) -> Self {
        let tx_man = SqliteTxManager::new(pool);
        let repository = SqliteRepository;
        let transfer_usecase = TransferUsecase::new(tx_man.clone(), repository.clone(), repository.clone());
        let get_balance_usecase = GetBalanceUsecase::new(tx_man, repository);
        Self {
            transfer_usecase,
            get_balance_usecase
        }
    }
}
