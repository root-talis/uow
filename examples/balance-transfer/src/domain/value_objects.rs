use std::num::NonZeroU32;

use derive_more::Display;

#[derive(Debug, Display, Clone, Copy)]
#[display("{_0}")]
pub struct AccountNumber(NonZeroU32);
impl AccountNumber {
    pub fn new(n: NonZeroU32) -> Self {
        Self(n)
    }

    pub fn inner(&self) -> &NonZeroU32 {
        &self.0
    }
}

/// Amount of money in some fictional currency
#[derive(Debug, Display, Clone, Copy)]
#[display("{_0}")]
pub struct CreditsAmount(u32);
impl CreditsAmount {
    pub fn new(a: u32) -> Self {
        Self(a)
    }

    pub fn inner(&self) -> &u32 {
        &self.0
    }
}
