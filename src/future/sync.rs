pub trait SyncFuture<T> {
    /// Get the value from the future, blocking if necessary.
    fn take(self) -> T;

    /// Gets the value from the future if it has been completed.
    fn try_take(self) -> Result<T, Self>;
}
