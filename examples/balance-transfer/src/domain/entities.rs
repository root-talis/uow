use crate::domain::{
    errors::WithdrawFailure,
    value_objects::{AccountNumber, CreditsAmount},
};

#[derive(Debug)]
pub struct Account {
    number: AccountNumber,
    balance: CreditsAmount,
}

impl Account {
    pub fn new(number: AccountNumber, balance: CreditsAmount) -> Self {
        Self { number, balance }
    }

    pub fn number(&self) -> &AccountNumber {
        &self.number
    }

    pub fn balance(&self) -> &CreditsAmount {
        &self.balance
    }

    pub fn deposit(&mut self, amount: CreditsAmount) {
        self.balance = CreditsAmount::new(self.balance.inner() + amount.inner());
    }

    pub fn withdraw(&mut self, amount: CreditsAmount) -> Result<(), WithdrawFailure> {
        if self.balance.inner() < amount.inner() {
            return Err(WithdrawFailure::InsufficientFunds);
        }
        self.balance = CreditsAmount::new(self.balance.inner() - amount.inner());
        Ok(())
    }
}
