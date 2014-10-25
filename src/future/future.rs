use std::time::Duration;
use super::{val, FutureVal};

pub trait Cancel {
    /// If not already completed, signals that the consumer is no longer
    /// interested in the result of the future.
    fn cancel(self);
}

// TODO:
// - Future::receive should return a Cancel that allows canceling the callback registration
//     Gated on associated type bugs
//     - https://github.com/rust-lang/rust/issues/18178
//     - https://github.com/rust-lang/rust/issues/17956
//
// - Future transformation fns should return generic futures and not be hard coded to FutureVal,
// but this also required working associated types as well as default fns.

pub trait Future<T: Send> : Send {
    /// When the future is complete, call the supplied function with the
    /// value.
    fn receive<F: Send + FnOnce(FutureResult<T>)>(self, f: F);

    /// Maps a FutureVal<T>  to FutureVal<U> by applying a function to the value once it is
    /// realized.
    fn map<U: Send, F: Send + FnOnce(T) -> U>(self, f: F) -> FutureVal<U> {
        let (ret, producer) = val::future::<U>();

        producer.receive(move |:p: FutureResult<val::Producer<U>>| {
            if let Ok(p) = p {
                self.receive(move |:val: FutureResult<T>| {
                    match val {
                        Ok(v) => p.complete(f(v)),
                        Err(e) => p.fail(e.desc),
                    }
                });
            }
        });

        ret
    }

    fn and<U: Send, F: Future<U>>(self, _fut: F) -> FutureVal<U> {
        unimplemented!()
    }

    fn and_then<U: Send, F: Future<U>, Fn: Send + FnOnce(T) -> F>(self, f: Fn) -> FutureVal<U> {
        let (ret, producer) = val::future::<U>();

        // Wait for the consumer to express interest in the final future
        producer.receive(move |:p: FutureResult<val::Producer<U>>| {
            if let Ok(p) = p {
                // Register a callback to handle the original future
                self.receive(move |:val: FutureResult<T>| {
                    match val {
                        Ok(v) => {
                            // If the value is successful, pass it to the supplied function and
                            // then register a callback to handle the new future
                            f(v).receive(move |:val: FutureResult<U>| {
                                match val {
                                    // If successful, complete the final future with the received
                                    // result.
                                    Ok(v) => p.complete(v),
                                    Err(e) => p.fail(e.desc),
                                }
                            });
                        }
                        Err(e) => p.fail(e.desc),
                    }
                });
            }
        });

        ret
    }

    fn or<T: Send, F: Future<T>>(self, _fut: F) -> FutureVal<T> {
        unimplemented!()
    }

    fn or_else<F: Future<T>, Fn: Send + FnOnce(FutureError) -> T>(self, _f: Fn) -> FutureVal<T> {
        unimplemented!()
    }
}

pub trait SyncFuture<T> {
    /// Get the value from the future, blocking if necessary.
    fn take(self) -> FutureResult<T>;

    fn take_timed(self, timeout: Duration) -> FutureResult<T>;
}

pub type FutureResult<T> = Result<T, FutureError>;

#[deriving(Show)]
pub struct FutureError {
    pub kind: FutureErrorKind,
    pub desc: &'static str,
}

impl FutureError {
    pub fn is_cancelation_error(&self) -> bool {
        match self.kind {
            CancelationError => true,
            _ => false,
        }
    }

    pub fn is_execution_error(&self) -> bool {
        match self.kind {
            ExecutionError => true,
            _ => false,
        }
    }
}

#[deriving(Show)]
pub enum FutureErrorKind {
    ExecutionError,
    CancelationError,
}

#[cfg(test)]
mod test {
    use future::{val, Future, FutureVal};

    #[test]
    pub fn test_and() {
    }
}
