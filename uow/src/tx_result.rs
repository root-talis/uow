/// Transaction result that indicates whether the transaction should be committed or rolled back.
pub enum TxResult<T> {
    /// Commit the transaction and return the contained value.
    Commit(T),
    /// Rollback the transaction and return the contained value.
    Rollback(T),
}

impl<T> TxResult<T> {
    /// Create a `Commit` variant.
    pub fn commit(r: T) -> Self {
        Self::Commit(r)
    }

    /// Create a `Rollback` variant.
    pub fn rollback(r: T) -> Self {
        Self::Rollback(r)
    }
}

#[cfg(test)]
mod tests {
    use super::TxResult;

    #[tokio::test]
    async fn test_tx_result_convenience_constructors() {
        let commit_result: TxResult<i32> = TxResult::commit(42);
        let rollback_result: TxResult<i32> = TxResult::rollback(42);

        match commit_result {
            TxResult::Commit(v) => assert_eq!(v, 42),
            TxResult::Rollback(_) => panic!("Expected Commit"),
        }

        match rollback_result {
            TxResult::Rollback(v) => assert_eq!(v, 42),
            TxResult::Commit(_) => panic!("Expected Rollback"),
        }
    }
}
