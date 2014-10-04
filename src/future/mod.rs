pub use self::future::Future;
pub use self::pair::{PairedFuture, Completer, pair};
pub use self::sync::SyncFuture;

mod future;
mod pair;
mod sync;
