pub use self::future::{
    Cancel,
    Future,
    SyncFuture,
    FutureResult,
    FutureError,
    FutureErrorKind,
    ExecutionError,
    CancelationError,
};
pub use self::join::{
    join,
    join_all,
};
pub use self::val::{
    FutureVal,
    future,
};

mod future;
mod join;
pub mod val;
