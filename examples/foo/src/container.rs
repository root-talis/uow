use sqlx::SqlitePool;

use crate::{application::TransferUsecase, infrastructure::SqlxTransferRepository};

#[derive(Clone)]
pub struct Container {
    pub transfer_usecase: TransferUsecase<SqlxTransferRepository>,
}

impl Container {
    pub fn new(pool: SqlitePool) -> Self {
        let uow_fac = super::UoWFac::new(pool);
        let repository = SqlxTransferRepository;
        let transfer_usecase = TransferUsecase::new(uow_fac, repository);
        Self {
            transfer_usecase,
        }
    }
}
