mod uow;
pub use uow::*;

#[cfg(feature = "test-utils")]
pub mod test_utils;

#[cfg(feature = "sqlx")]
pub mod sqlx;

/// Convenience macro for [`TxManager::transaction`].
///
/// This macro eliminates the boilerplate of wrapping business logic in an inner
/// `async` block and returning the Unit of Work together with the result.
///
/// # Syntax
///
/// ```ignore
/// transaction!(tx_manager_expr, |mut uow| async {
///     // business logic with ? operator
///     Ok(result)
/// })
/// ```
///
/// The `tx_manager_expr` is any expression that evaluates to something implementing
/// [`TxManager`]. The closure pattern (`|mut uow|` or `|uow|`) is passed through
/// verbatim.
///
/// # `move` keyword
///
/// By default, the outer async closure captures surrounding variables by reference.
/// Add `move` to capture by move instead:
///
/// ```ignore
/// transaction!(tx_manager_expr, |mut uow| async move {
///     // business logic with ? operator
///     Ok(result)
/// })
/// ```
///
/// **Important:** In both variants, the *inner* async block is always non-`move`
/// (`async { ... }`). This ensures the Unit of Work is borrowed, not consumed,
/// so it remains available after the inner block completes for the tuple return
/// `($uow, r)`.
///
/// # Why the inner block must borrow `uow`
///
/// The macro expands to code that evaluates the user's body inside an inner
/// `async { ... }` block, then awaits it and returns `($uow, r)`:
///
/// ```ignore
/// tx.transaction(|uow| async {
///     let r = async { /* user body */ }.await;
///     (uow, r)          // <-- uow is used here
/// })
/// ```
///
/// If the inner block were `async move { ... }`, it would take ownership of `uow`
/// and the final `(uow, r)` tuple would be impossible to construct because `uow`
/// would have been moved into the block. By keeping the inner block non-`move`,
/// `uow` is only *borrowed* for the duration of the `await`, and ownership flows
/// back to the closure so it can be returned.
///
/// # Example
///
/// ```rust,ignore
/// use uow::transaction;
///
/// let result = transaction!(tx_manager, |mut uow| async {
///     let user = user_repo.create(&mut uow, "alice").await?;
///     account_repo.create(&mut uow, user.id).await?;
///     Ok(user)
/// }).await;
/// ```
///
/// # Errors
///
/// Errors returned inside the transaction body are propagated through the
/// [`TxManager::transaction`] call. The exact semantics (rollback, commit, etc.)
/// depend on the [`TxManager`] implementation.
///
/// # Panics
///
/// This macro itself does not panic. Panics may occur only if the underlying
/// [`TxManager::transaction`] implementation panics or if the user-provided
/// body panics.
#[macro_export]
macro_rules! transaction {
    // Variant without `move`: captures surrounding variables by reference.
    ($tx:expr, |mut $uow:ident| async { $($body:tt)* }) => {
        $tx.transaction(|mut $uow| async {
            let r = async { $($body)* }.await;
            ($uow, r)
        })
    };
    // Variant with `move`: captures surrounding variables by move.
    ($tx:expr, |mut $uow:ident| async move { $($body:tt)* }) => {
        $tx.transaction(|mut $uow| async move {
            let r = async { $($body)* }.await;
            ($uow, r)
        })
    };
}
