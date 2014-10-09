//! Implementes an unbuffered Stream of elements.
#![allow(dead_code)]
#![allow(unused_variable)]
#![allow(unused_imports)]

use std::mem;
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

impl<T: Send> Future<Option<(T, Seq<T>)>> for Seq<T> {
    fn is_complete(&self) -> bool {
        unimplemented!();
    }

    fn receive<F: FnOnce(Option<(T, Seq<T>)>) -> ()>(self, cb: F) {
        unimplemented!();
    }
}

impl<T: Send> SyncFuture<Option<(T, Seq<T>)>> for Seq<T> {
    fn take(self) -> Option<(T, Seq<T>)> {
        let mut head;

        {
            let mut l = self.core.lock();
            l.completion = Wait;

            loop {
                if let Some(h) = l.take_head() {
                    head = h;
                    break;
                }

                l.wait(&l.condvar);
            }
        }

        match head {
            More(v) => Some((v, self)),
            Done => None
        }
    }

    /// Gets the value from the future if it has been completed.
    fn try_take(self) -> Result<Option<(T, Seq<T>)>, Seq<T>> {
        unimplemented!()
    }
}

pub struct SeqProducer<T> {
    core: Arc<MutexCell<Core<T>>>,
}

impl<T: Send> SeqProducer<T> {
    pub fn send(self, val: T) {
        let mut l = self.core.lock();

        if let Callback(cb) = l.take_callback() {
            drop(l);

            // The rest of the stream
            let rest = Seq { core: self.core.clone() };
            cb.call_once((Some((val, rest)),));
            return;
        }

        l.put(val);

        if l.completion.is_wait() {
            l.condvar.signal();
        }
    }
}

struct Core<T> {
    head: Option<Head<T>>,
    condvar: CondVar,
    completion: Completion<T>,
}

impl<T: Send> Core<T> {
    fn new() -> Core<T> {
        Core {
            head: None,
            condvar: CondVar::new(),
            completion: Pending,
        }
    }

    fn put(&mut self, val: T) {
        assert!(self.head.is_none(), "stream not ready for next value");
        self.head = Some(More(val));
    }

    fn take_head(&mut self) -> Option<Head<T>> {
        mem::replace(&mut self.head, None)
    }

    fn take_callback(&mut self) -> Completion<T> {
        if self.completion.is_callback() {
            mem::replace(&mut self.completion, Pending)
        } else {
            Pending
        }
    }
}

enum Head<T> {
    More(T),
    Done,
}

enum Completion<T> {
    Pending,
    Wait,
    Callback(Box<FnOnce<(Option<(T, Seq<T>)>,), ()> + Send>),
}

impl<T: Send> Completion<T> {
    fn is_pending(&self) -> bool {
        match *self {
            Pending => true,
            _ => false,
        }
    }

    fn is_wait(&self) -> bool {
        match *self {
            Wait => true,
            _ => false,
        }
    }

    fn is_callback(&self) -> bool {
        match *self {
            Callback(..) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod test {
    use std::io::timer::sleep;
    use std::time::Duration;
    use future::{Future, SyncFuture};
    use super::*;

    #[test]
    pub fn test_get_head() {
        let (stream, producer) = seq();

        spawn(proc() {
            sleep(Duration::milliseconds(50));
            producer.send("hello");
        });

        if let Some((v, rest)) = stream.take() {
            assert_eq!(v, "hello");
        } else {
            fail!("nope");
        }
    }
}
