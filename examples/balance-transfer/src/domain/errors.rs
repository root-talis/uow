use thiserror::Error;

#[derive(Error, Debug)]
pub enum WithdrawFailure {
    #[error("insufficient funds")]
    InsufficientFunds,
}

#[derive(Error, Debug)]
pub enum TransferFailure {
    #[error("failed to withdraw credits for transfer: {_0}")]
    WithdrawFailure(#[source] #[from] WithdrawFailure)
}
