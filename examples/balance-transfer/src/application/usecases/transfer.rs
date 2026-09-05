use thiserror::Error;
use tracing::error;

use crate::application::ports::{BalanceReadError, BalanceRepository, TransferRepository};

use uow::{IsloationLevelNotSupportedError, IsolationLevel, IsolationLevelSelector, TxManager, UoW};

pub struct TransferRequest {
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub amount: u32,
}

pub type TransferResult = ();

#[derive(Error, Debug)]
pub enum TransferError<U: UoW> {
    #[error("source account has insufficient funds")]
    InsufficientFunds {src_user_id: i64},

    #[error("platform doesnt support isolation level")]
    IsolationLevelNotSupported(#[from] IsloationLevelNotSupportedError),

    #[error("persistence error")]
    PersistenceError(#[source] U::Error),

    #[error(transparent)]
    BalanceReadError(#[from] BalanceReadError<U>),

    #[error("account not available: {user_id}")]
    AccountNotAvailable{ user_id: i64 },
}

#[derive(Clone)]
pub struct TransferUsecase<M, TrR, BaR> 
where
    M: TxManager,
    TrR: TransferRepository<M::UoW>,
    BaR: BalanceRepository<M::UoW>
{
    tx_manager: M,
    transfer_repo: TrR,
    balance_repo: BaR,
}

impl<M, TrR, BaR> TransferUsecase<M, TrR, BaR>
where
    M: TxManager,
    TrR: TransferRepository<M::UoW>,
    BaR: BalanceRepository<M::UoW>
{
    pub fn new(tx_manager: M, transfer_repo: TrR, balance_repo: BaR) -> Self {
        Self { tx_manager, transfer_repo, balance_repo }
    }

    pub async fn execute(&self, request: TransferRequest) -> Result<TransferResult, TransferError<M::UoW>> {
        self.tx_manager
            .isolated_at(IsolationLevel::RepeatableRead)
                .or(IsolationLevel::Serializable)
                .if_platform_supports()?
            .transaction(|mut uow| async {
                let r = async {
                    let src_balance = self.balance_repo
                        .read_balance(&mut uow, request.from_user_id)
                        .await?
                        .ok_or(TransferError::AccountNotAvailable { user_id: request.from_user_id })?
                    ;

                    if src_balance < request.amount {
                        return Err(TransferError::InsufficientFunds { src_user_id: request.from_user_id })
                    }

                    self.balance_repo
                        .read_balance(&mut uow, request.to_user_id)
                        .await?
                        .ok_or(TransferError::AccountNotAvailable { user_id: request.to_user_id })?
                    ;

                    self.transfer_repo
                        .withdraw(&mut uow, request.from_user_id, request.amount)
                        .await
                        .map_err(TransferError::PersistenceError)?;

                    self.transfer_repo
                        .deposit(&mut uow, request.to_user_id, request.amount)
                        .await
                        .map_err(TransferError::PersistenceError)?;

                    Ok(())
                }.await;

                (uow, r)
            })
            .await
            .map_err(|e| {
                error!(e = e.to_string(), "transaction error");
                TransferError::PersistenceError(e.into_inner())
            })?
    }
}
