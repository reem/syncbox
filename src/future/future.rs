pub trait Future<T> {
    /// When the future is complete, call the supplied function with the
    /// value.
    fn receive<F: FnOnce(T) -> () + Send>(self, cb: F);
}

pub trait SyncFuture<T> {
    /// Get the value from the future, blocking if necessary.
    fn take(self) -> T;
}
