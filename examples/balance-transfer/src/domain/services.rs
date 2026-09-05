use crate::domain::{entities::Account, errors::TransferFailure, value_objects::CreditsAmount};

pub fn transfer_credits(from: &mut Account, amount: CreditsAmount, to: &mut Account) -> Result<(), TransferFailure> {
    from.withdraw(amount)?;
    to.deposit(amount);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;
    use crate::domain::{errors::WithdrawFailure, value_objects::AccountNumber};

    fn account(number: u32, balance: u32) -> Account {
        Account::new(
            AccountNumber::new(NonZeroU32::new(number).unwrap()),
            CreditsAmount::new(balance),
        )
    }

    #[test]
    fn transfer_should_decrease_sender_balance() {
        let mut sender = account(1, 100);
        let mut receiver = account(2, 50);

        transfer_credits(&mut sender, CreditsAmount::new(30), &mut receiver).unwrap();

        assert_eq!(*sender.balance().inner(), 70);
    }

    #[test]
    fn transfer_should_increase_receiver_balance() {
        let mut sender = account(1, 100);
        let mut receiver = account(2, 50);

        transfer_credits(&mut sender, CreditsAmount::new(30), &mut receiver).unwrap();

        assert_eq!(*receiver.balance().inner(), 80);
    }

    #[test]
    fn transfer_zero_amount_should_leave_sender_balance_unchanged() {
        let mut sender = account(1, 100);
        let mut receiver = account(2, 50);

        transfer_credits(&mut sender, CreditsAmount::new(0), &mut receiver).unwrap();

        assert_eq!(*sender.balance().inner(), 100);
    }

    #[test]
    fn transfer_zero_amount_should_leave_receiver_balance_unchanged() {
        let mut sender = account(1, 100);
        let mut receiver = account(2, 50);

        transfer_credits(&mut sender, CreditsAmount::new(0), &mut receiver).unwrap();

        assert_eq!(*receiver.balance().inner(), 50);
    }

    #[test]
    fn transfer_insufficient_funds_should_return_error() {
        let mut sender = account(1, 20);
        let mut receiver = account(2, 50);

        let result = transfer_credits(&mut sender, CreditsAmount::new(30), &mut receiver);

        assert!(matches!(
            result,
            Err(TransferFailure::WithdrawFailure(WithdrawFailure::InsufficientFunds))
        ));
    }

    #[test]
    fn transfer_insufficient_funds_should_leave_sender_balance_unchanged() {
        let mut sender = account(1, 20);
        let mut receiver = account(2, 50);

        let _ = transfer_credits(&mut sender, CreditsAmount::new(30), &mut receiver);

        assert_eq!(*sender.balance().inner(), 20);
    }

    #[test]
    fn transfer_insufficient_funds_should_leave_receiver_balance_unchanged() {
        let mut sender = account(1, 20);
        let mut receiver = account(2, 50);

        let _ = transfer_credits(&mut sender, CreditsAmount::new(30), &mut receiver);

        assert_eq!(*receiver.balance().inner(), 50);
    }
}
