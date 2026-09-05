use sqlx::SqlitePool;

use crate::application::{GetBalanceUsecase, TransferUsecase};
use crate::infrastructure::SqlxRepository;
use crate::uow_sqlx::SqlxUoWFac;

#[derive(Clone)]
pub struct Container {
    pub transfer_usecase: TransferUsecase<SqlxUoWFac, SqlxRepository, SqlxRepository>,
    pub get_balance_usecase: GetBalanceUsecase<SqlxUoWFac, SqlxRepository>,
}

impl Container {
    pub fn new(pool: SqlitePool) -> Self {
        let uow_fac = SqlxUoWFac::new(pool);
        let repository = SqlxRepository;
        let transfer_usecase = TransferUsecase::new(uow_fac.clone(), repository.clone(), repository.clone());
        let get_balance_usecase = GetBalanceUsecase::new(uow_fac, repository);
        Self {
            transfer_usecase,
            get_balance_usecase
        }
    }
}
