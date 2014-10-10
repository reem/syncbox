//! A basic implementation of Future.
//!
//! As of now, the implementation is fairly naive, using a mutex to
//! handle synchronization. However, this will eventually be
//! re-implemented using lock free strategies once the API stabalizes.

use std::{fmt, mem};
use sync::{Arc, MutexCell, CondVar};
use super::{Future, SyncFuture};

pub fn future<T: Send>() -> (FutureVal<T>, Completer<T>) {
    let core = Arc::new(MutexCell::new(Core::new()));

    let f = FutureVal { core: core.clone() };
    let c = Completer { core: core };

    (f, c)
}

pub struct FutureVal<T> {
    core: Arc<MutexCell<Core<T>>>,
}

impl<T: Send> Future<T> for FutureVal<T> {
    fn is_complete(&self) -> bool {
        let mut l = self.core.lock();
        !l.completion.is_pending()
    }

    fn receive<F: FnOnce(T) -> () + Send>(self, cb: F) {
        let mut l = self.core.lock();

        if let CompleterCb(cb) = l.take_completer_cb() {
            drop(l);
            cb.call_once((Completer { core: self.core.clone() },));
            l = self.core.lock();
        }

        if let Some(v) = l.take_val() {
            drop(l); // Escape the mutex
            cb(v);
            return;
        }

        l.completion = ConsumerCb(box cb);
    }
}

impl<T: Send> SyncFuture<T> for FutureVal<T> {
    fn take(self) -> T {
        let mut l = self.core.lock();

        if let CompleterCb(cb) = l.take_completer_cb() {
            drop(l);
            cb.call_once((Completer { core: self.core.clone() },));
            l = self.core.lock();
        } else {
            if l.completion.is_completer_wait() {
                l.condvar.signal();
            }
        }

        l.completion = ConsumerWait;

        loop {
            if let Some(v) = l.take_val() {
                return v;
            }

            l.wait(&l.condvar);
        }
    }

    fn try_take(self) -> Result<T, FutureVal<T>> {
        {
            let mut l = self.core.lock();

            if let Some(v) = l.take_val() {
                return Ok(v);
            }
        }

        Err(self)
    }
}

impl<T: fmt::Show> fmt::Show for FutureVal<T> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        try!(write!(fmt, "FutureVal"));
        Ok(())
    }
}

pub struct Completer<T> {
    core: Arc<MutexCell<Core<T>>>,
}

impl<T: Send> Completer<T> {
    pub fn complete(self, val: T) {
        let mut l = self.core.lock();

        if let ConsumerCb(cb) = l.take_consumer_cb() {
            drop(l);
            cb.call_once((val,));
            return;
        }

        l.put(val);

        if l.completion.is_consumer_wait() {
            l.condvar.signal();
        }
    }
}

/*
 *
 * ===== Consumer interest future =====
 *
 */

impl<T: Send> Future<Completer<T>> for Completer<T> {
    fn is_complete(&self) -> bool {
        let mut l = self.core.lock();
        !l.completion.is_pending()
    }

    fn receive<F: FnOnce(Completer<T>) -> () + Send>(self, cb: F) {
        {
            let mut l = self.core.lock();

            if l.completion.is_pending() {
                l.completion = CompleterCb(box cb);
                return;
            }

            drop(l);
        }

        // The consumer is currently waiting, invoke the callback
        cb(self);
    }
}

impl<T: Send> SyncFuture<Completer<T>> for Completer<T> {
    fn take(self) -> Completer<T> {
        {
            let mut l = self.core.lock();

            if l.completion.is_pending() {
                l.completion = CompleterWait;

                loop {
                    l.wait(&l.condvar);

                    if !l.completion.is_completer_wait() {
                        break;
                    }
                }
            }
        }

        self
    }

    /// Gets the value from the future if it has been completed.
    fn try_take(self) -> Result<Completer<T>, Completer<T>> {
        if self.is_complete() {
            Ok(self)
        } else {
            Err(self)
        }
    }
}

struct Core<T> {
    val: Option<T>,
    condvar: CondVar,
    completion: Completion<T>,
}

impl<T: Send> Core<T> {
    fn new() -> Core<T> {
        Core {
            val: None,
            condvar: CondVar::new(),
            completion: Pending,
        }
    }

    fn put(&mut self, val: T) {
        assert!(self.val.is_none(), "future already completed");
        self.val = Some(val);
    }

    fn take_val(&mut self) -> Option<T> {
        mem::replace(&mut self.val, None)
    }

    fn take_consumer_cb(&mut self) -> Completion<T> {
        if self.completion.is_consumer_cb() {
            mem::replace(&mut self.completion, Pending)
        } else {
            Pending
        }
    }

    fn take_completer_cb(&mut self) -> Completion<T> {
        if self.completion.is_completer_cb() {
            mem::replace(&mut self.completion, Pending)
        } else {
            Pending
        }
    }
}

enum Completion<T> {
    Pending,
    ConsumerWait,
    CompleterWait,
    ConsumerCb(Box<FnOnce<(T,),()> + Send>),
    CompleterCb(Box<FnOnce<(Completer<T>,),()> + Send>),
}

impl<T: Send> Completion<T> {
    fn is_pending(&self) -> bool {
        match *self {
            Pending => true,
            _ => false,
        }
    }

    fn is_consumer_wait(&self) -> bool {
        match *self {
            ConsumerWait => true,
            _ => false,
        }
    }

    fn is_completer_wait(&self) -> bool {
        match *self {
            CompleterWait => true,
            _ => false,
        }
    }

    fn is_consumer_cb(&self) -> bool {
        match *self {
            ConsumerCb(..) => true,
            _ => false,
        }
    }

    fn is_completer_cb(&self) -> bool {
        match *self {
            CompleterCb(..) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod test {
    use std::io::timer::sleep;
    use std::time::Duration;
    use sync::Arc;
    use sync::atomic::{AtomicBool, Relaxed};
    use future::{Future, SyncFuture};
    use super::*;

    #[test]
    pub fn test_complete_before_take() {
        let (f, c) = future();

        spawn(proc() {
            c.complete("zomg");
        });

        sleep(Duration::milliseconds(50));
        assert_eq!(f.take(), "zomg");
    }

    #[test]
    pub fn test_complete_after_take() {
        let (f, c) = future();

        spawn(proc() {
            sleep(Duration::milliseconds(50));
            c.complete("zomg");
        });

        assert_eq!(f.take(), "zomg");
    }

    #[test]
    pub fn test_try_take_no_val() {
        let (f, _) = future::<uint>();
        assert!(f.try_take().is_err());
    }

    #[test]
    pub fn test_try_take_val() {
        let (f, c) = future();
        c.complete("hello");
        assert_eq!(f.try_take().unwrap(), "hello");
    }

    #[test]
    pub fn test_complete_before_receive() {
        let (f, c) = future();
        let (tx, rx) = channel::<&'static str>();

        spawn(proc() {
            c.complete("zomg");
        });

        sleep(Duration::milliseconds(50));
        f.receive(move |:v| tx.send(v));
        assert_eq!(rx.recv(), "zomg");
    }

    #[test]
    pub fn test_complete_after_receive() {
        let (f, c) = future();
        let (tx, rx) = channel::<&'static str>();

        spawn(proc() {
            sleep(Duration::milliseconds(50));
            c.complete("zomg");
        });

        f.receive(move |:v| tx.send(v));
        assert_eq!(rx.recv(), "zomg");
    }

    #[test]
    pub fn test_receive_complete_future_before_take() {
        let (f, c) = future::<&'static str>();
        let w1 = Arc::new(AtomicBool::new(false));
        let w2 = w1.clone();

        c.receive(move |:c: Completer<&'static str>| {
            assert!(w2.load(Relaxed));
            c.complete("zomg");
        });

        w1.store(true, Relaxed);
        assert_eq!(f.take(), "zomg");
    }

    #[test]
    pub fn test_receive_complete_future_after_take() {
        let (f, c) = future();
        let w1 = Arc::new(AtomicBool::new(false));
        let w2 = w1.clone();

        spawn(proc() {
            sleep(Duration::milliseconds(50));

            c.receive(move |:c: Completer<&'static str>| {
                assert!(w2.load(Relaxed));
                c.complete("zomg");
            });
        });

        w1.store(true, Relaxed);
        assert_eq!(f.take(), "zomg");
    }

    #[test]
    pub fn test_receive_complete_before_consumer_receive() {
        let (f, c) = future();
        let w1 = Arc::new(AtomicBool::new(false));
        let w2 = w1.clone();

        c.receive(move |:c: Completer<&'static str>| {
            assert!(w2.load(Relaxed));
            c.complete("zomg");
        });

        let (tx, rx) = channel();
        w1.store(true, Relaxed);

        f.receive(move |:msg| {
            assert_eq!("zomg", msg);
            tx.send("hi2u");
        });

        assert_eq!("hi2u", rx.recv());
    }

    #[test]
    pub fn test_receive_complete_after_consumer_receive() {
        let (f, c) = future();
        let w1 = Arc::new(AtomicBool::new(false));
        let w2 = w1.clone();

        spawn(proc() {
            sleep(Duration::milliseconds(50));

            c.receive(move |:c: Completer<&'static str>| {
                assert!(w2.load(Relaxed));
                c.complete("zomg");
            });
        });

        let (tx, rx) = channel();
        w1.store(true, Relaxed);

        f.receive(move |:msg| {
            assert_eq!("zomg", msg);
            tx.send("hi2u");
        });

        assert_eq!("hi2u", rx.recv());
    }

    #[test]
    pub fn test_take_complete_before_consumer_take() {
        let (f, c) = future();

        spawn(proc() {
            c.take().complete("zomg");
        });

        sleep(Duration::milliseconds(50));
        assert_eq!("zomg", f.take());
    }

    #[test]
    pub fn test_take_complete_after_consumer_take() {
        assert!(true);
    }

    #[test]
    pub fn test_take_complete_before_consumer_receive() {
        assert!(true);
    }

    #[test]
    pub fn test_take_complete_after_consumer_receive() {
        assert!(true);
    }

}
