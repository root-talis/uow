use crate::infrastructure::TransferRepository;
use crate::presentation::TransferRequest;
use crate::uow::UoWFac;

#[derive(Clone)]
pub struct TransferUsecase<R: TransferRepository + Clone> {
    uow_fac: UoWFac,
    repository: R,
}

impl<R: TransferRepository + Clone> TransferUsecase<R> {
    pub fn new(uow_fac: UoWFac, repository: R) -> Self {
        Self { uow_fac, repository }
    }

    pub async fn execute(
        &self,
        payload: &TransferRequest,
    ) -> Result<(), sqlx::Error> {
        let mut uow = self.uow_fac.create().await?;

        self.repository.withdraw(&mut uow, payload.from_user_id, payload.amount).await?;
        self.repository.deposit(&mut uow, payload.to_user_id, payload.amount).await?;
        
        uow.commit().await?;

        Ok(())
    }
}
