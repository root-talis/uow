use std::num::{NonZeroI64, NonZeroU32, TryFromIntError};

use axum::extract::FromRef;

use crate::domain::{
    entities::Account,
    value_objects::{AccountNumber, CreditsAmount},
};

/// Temporary struct to read account data from Sqlite db
pub struct AccountData {
    pub id: i64,
    pub balance: i64,
}

/// Convert sqlite account table data into Account entity
impl TryInto<Account> for AccountData {
    type Error = TryFromIntError;

    fn try_into(self) -> Result<Account, Self::Error> {
        Ok(Account::new(self.id.try_into()?, self.balance.try_into()?))
    }
}

impl FromRef<Account> for AccountData {
    fn from_ref(value: &Account) -> Self {
        Self {
            id: FromRef::from_ref(value.number()),
            balance: FromRef::from_ref(value.balance()),
        }
    }
}

/// Read AccountNumber from the way it is stored in Sqlite
impl TryInto<AccountNumber> for i64 {
    type Error = TryFromIntError;

    fn try_into(self) -> Result<AccountNumber, Self::Error> {
        let accid: NonZeroI64 = self.try_into()?;
        let accid: NonZeroU32 = accid.try_into()?;
        Ok(AccountNumber::new(accid))
    }
}

impl FromRef<AccountNumber> for i64 {
    fn from_ref(value: &AccountNumber) -> Self {
        let u32: u32 = (*value.inner()).into();
        u32.into()
    }
}

/// Read CreditsAmount from the way it is stored in Sqlite
impl TryInto<CreditsAmount> for i64 {
    type Error = TryFromIntError;

    fn try_into(self) -> Result<CreditsAmount, Self::Error> {
        Ok(CreditsAmount::new(self.try_into()?))
    }
}

impl FromRef<CreditsAmount> for i64 {
    fn from_ref(value: &CreditsAmount) -> Self {
        (*value.inner()).into()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use crate::domain::value_objects::{AccountNumber, CreditsAmount};

    fn account_number(n: u32) -> AccountNumber {
        AccountNumber::new(NonZeroU32::new(n).unwrap())
    }

    fn credits_amount(a: u32) -> CreditsAmount {
        CreditsAmount::new(a)
    }

    mod account_data {
        use std::num::NonZeroU32;

        use axum::extract::FromRef;

        use crate::domain::entities::Account;

        use super::super::AccountData;
        use super::{account_number, credits_amount};

        #[test]
        fn try_into_account_maps_valid_fields() {
            let data = AccountData {
                id: 42,
                balance: 100,
            };
            let account: Account = data.try_into().unwrap();
            assert_eq!(*account.number().inner(), NonZeroU32::new(42).unwrap());
            assert_eq!(*account.balance().inner(), 100);
        }

        #[test]
        fn try_into_account_rejects_zero_id() {
            let data = AccountData {
                id: 0,
                balance: 100,
            };
            let result: Result<Account, _> = data.try_into();
            assert!(result.is_err());
        }

        #[test]
        fn try_into_account_rejects_negative_id() {
            let data = AccountData {
                id: -1,
                balance: 100,
            };
            let result: Result<Account, _> = data.try_into();
            assert!(result.is_err());
        }

        #[test]
        fn try_into_account_rejects_negative_balance() {
            let data = AccountData {
                id: 42,
                balance: -1,
            };
            let result: Result<Account, _> = data.try_into();
            assert!(result.is_err());
        }

        #[test]
        fn try_into_account_accepts_u32_max_fields() {
            let data = AccountData {
                id: u32::MAX as i64,
                balance: u32::MAX as i64,
            };
            let account: Account = data.try_into().unwrap();
            assert_eq!(
                *account.number().inner(),
                NonZeroU32::new(u32::MAX).unwrap()
            );
            assert_eq!(*account.balance().inner(), u32::MAX);
        }

        #[test]
        fn from_ref_account_extracts_fields() {
            let account = Account::new(account_number(42), credits_amount(100));
            let data = AccountData::from_ref(&account);
            assert_eq!(data.id, 42);
            assert_eq!(data.balance, 100);
        }
    }

    mod account_number {
        use std::num::NonZeroU32;

        use axum::extract::FromRef;

        use crate::domain::value_objects::AccountNumber;

        use super::account_number;

        #[test]
        fn i64_try_into_accepts_valid_positive() {
            let result: Result<AccountNumber, _> = 42i64.try_into();
            assert!(result.is_ok());
            assert_eq!(*result.unwrap().inner(), NonZeroU32::new(42).unwrap());
        }

        #[test]
        fn i64_try_into_accepts_u32_max() {
            let result: Result<AccountNumber, _> = (u32::MAX as i64).try_into();
            assert!(result.is_ok());
            assert_eq!(*result.unwrap().inner(), NonZeroU32::new(u32::MAX).unwrap());
        }

        #[test]
        fn i64_try_into_rejects_zero() {
            let result: Result<AccountNumber, _> = 0i64.try_into();
            assert!(result.is_err());
        }

        #[test]
        fn i64_try_into_rejects_negative() {
            let result: Result<AccountNumber, _> = (-1i64).try_into();
            assert!(result.is_err());
        }

        #[test]
        fn i64_try_into_rejects_u32_overflow() {
            let result: Result<AccountNumber, _> = (u32::MAX as i64 + 1).try_into();
            assert!(result.is_err());
        }

        #[test]
        fn i64_try_into_rejects_i64_max() {
            let result: Result<AccountNumber, _> = i64::MAX.try_into();
            assert!(result.is_err());
        }

        #[test]
        fn from_ref_i64_produces_correct_value() {
            let acc_num = account_number(42);
            let value = i64::from_ref(&acc_num);
            assert_eq!(value, 42);
        }

        #[test]
        fn from_ref_i64_roundtrips_u32_max() {
            let acc_num = account_number(u32::MAX);
            let value = i64::from_ref(&acc_num);
            assert_eq!(value, u32::MAX as i64);
            let roundtrip: Result<AccountNumber, _> = value.try_into();
            assert!(roundtrip.is_ok());
            assert_eq!(
                *roundtrip.unwrap().inner(),
                NonZeroU32::new(u32::MAX).unwrap()
            );
        }
    }

    mod credits_amount {
        use axum::extract::FromRef;

        use crate::domain::value_objects::CreditsAmount;

        use super::credits_amount;

        #[test]
        fn i64_try_into_accepts_valid_positive() {
            let result: Result<CreditsAmount, _> = 100i64.try_into();
            assert!(result.is_ok());
            assert_eq!(*result.unwrap().inner(), 100);
        }

        #[test]
        fn i64_try_into_accepts_u32_max() {
            let result: Result<CreditsAmount, _> = (u32::MAX as i64).try_into();
            assert!(result.is_ok());
            assert_eq!(*result.unwrap().inner(), u32::MAX);
        }

        #[test]
        fn i64_try_into_rejects_negative() {
            let result: Result<CreditsAmount, _> = (-1i64).try_into();
            assert!(result.is_err());
        }

        #[test]
        fn i64_try_into_rejects_u32_overflow() {
            let result: Result<CreditsAmount, _> = (u32::MAX as i64 + 1).try_into();
            assert!(result.is_err());
        }

        #[test]
        fn i64_try_into_rejects_i64_max() {
            let result: Result<CreditsAmount, _> = i64::MAX.try_into();
            assert!(result.is_err());
        }

        #[test]
        fn from_ref_i64_produces_correct_value() {
            let amount = credits_amount(100);
            let value = i64::from_ref(&amount);
            assert_eq!(value, 100);
        }

        #[test]
        fn from_ref_i64_roundtrips_u32_max() {
            let amount = credits_amount(u32::MAX);
            let value = i64::from_ref(&amount);
            assert_eq!(value, u32::MAX as i64);
            let roundtrip: Result<CreditsAmount, _> = value.try_into();
            assert!(roundtrip.is_ok());
            assert_eq!(*roundtrip.unwrap().inner(), u32::MAX);
        }
    }
}
