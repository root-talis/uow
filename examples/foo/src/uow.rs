use async_trait::async_trait;

/// Trait for a Unit of Work (transaction wrapper).
/// Allows different transaction backends to be plugged in.
#[async_trait]
pub trait UoW: Send {
    /// The error type returned by this UoW implementation.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Commit the transaction.
    async fn commit(self) -> Result<(), Self::Error>;
}

/// Trait for a Unit of Work factory.
/// Creates UoW instances with a specific error type.
#[async_trait]
pub trait UoWFac: Clone + Send + Sync {
    /// The type of UoW this factory creates.
    type UoW: UoW;

    /// Create a new UoW instance.
    async fn create(&self) -> Result<Self::UoW, <Self::UoW as UoW>::Error>;
}
