use sqlx::SqlitePool;

use crate::application::TransferUsecase;
use crate::infrastructure::SqlxTransferRepository;
use crate::uow_sqlx::SqlxUoWFac;

#[derive(Clone)]
pub struct Container {
    pub transfer_usecase: TransferUsecase<SqlxUoWFac, SqlxTransferRepository>,
}

impl Container {
    pub fn new(pool: SqlitePool) -> Self {
        let uow_fac = SqlxUoWFac::new(pool);
        let repository = SqlxTransferRepository;
        let transfer_usecase = TransferUsecase::new(uow_fac, repository);
        Self {
            transfer_usecase,
        }
    }
}
