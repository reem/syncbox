//! Implementes an unbuffered Stream of elements.

use sync::{Arc, MutexCell, CondVar};
use super::{Future, SyncFuture};

pub fn seq<T: Send>() -> (Seq<T>, SeqProducer<T>) {
    let core = Arc::new(MutexCell::new(Core::new()));

    let s = Seq { core: core.clone() };
    let p = SeqProducer { core: core };

    (s, p)
}

pub struct Seq<T> {
    core: Arc<MutexCell<Core<T>>>,
}

impl<T: Send> Future<(T, Option<Seq<T>>)> for Seq<T> {
    fn is_complete(&self) -> bool {
        unimplemented!();
    }

    fn receive<F: FnOnce((T, Option<Seq<T>>)) -> ()>(self, cb: F) {
        unimplemented!();
    }
}

impl<T: Send> SyncFuture<(T, Option<Seq<T>>)> for Seq<T> {
    fn take(self) -> (T, Option<Seq<T>>) {
        unimplemented!()
    }

    /// Gets the value from the future if it has been completed.
    fn try_take(self) -> Result<(T, Option<Seq<T>>), Seq<T>> {
        unimplemented!()
    }
}

pub struct SeqProducer<T> {
    core: Arc<MutexCell<Core<T>>>,
}

struct Core<T> {
    condvar: CondVar,
}

impl<T: Send> Core<T> {
    fn new() -> Core<T> {
        Core {
            condvar: CondVar::new(),
        }
    }
}
