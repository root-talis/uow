use crate::infrastructure::TransferRepository;
use crate::presentation::TransferRequest;
use crate::uow::{UoW, UoWFac};

#[derive(Clone)]
pub struct TransferUsecase<F: UoWFac, R: TransferRepository<F::UoW> + Clone> {
    uow_fac: F,
    repository: R,
}

impl<F: UoWFac, R: TransferRepository<F::UoW> + Clone> TransferUsecase<F, R> {
    pub fn new(uow_fac: F, repository: R) -> Self {
        Self { uow_fac, repository }
    }

    pub async fn execute(
        &self,
        payload: &TransferRequest,
    ) -> Result<(), <F::UoW as crate::uow::UoW>::Error> {
        let mut uow = self.uow_fac.create().await?;

        self.repository.withdraw(&mut uow, payload.from_user_id, payload.amount).await?;
        self.repository.deposit(&mut uow, payload.to_user_id, payload.amount).await?;
        
        uow.commit().await?;

        Ok(())
    }
}
