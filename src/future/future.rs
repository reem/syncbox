pub trait Future<T> {
    /// Whether or not the future is complete
    fn is_complete(&self) -> bool;

    /// When the future is complete, call the supplied function with the
    /// value.
    fn receive<F: FnOnce<(T,), ()> + Send>(self, cb: F);
}

pub trait SyncFuture<T> {
    /// Get the value from the future, blocking if necessary.
    fn take(self) -> T;

    /// Gets the value from the future if it has been completed.
    fn try_take(self) -> Result<T, Self>;
}
