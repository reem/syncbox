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

    let f = FutureVal::new(core.clone());
    let c = Completer::new(core);

    (f, c)
}

pub struct FutureVal<T> {
    core: Arc<MutexCell<Core<T>>>,
}

impl<T: Send> FutureVal<T> {

    // Initializes a new FutureVal with the given core
    fn new(core: Arc<MutexCell<Core<T>>>) -> FutureVal<T> {
        FutureVal { core: core }
    }

    pub fn is_complete(&self) -> bool {
        let mut l = self.core.lock();
        !l.completion.is_pending()
    }

    pub fn try_take(self) -> Result<T, FutureVal<T>> {
        {
            let mut l = self.core.lock();

            if let Some(v) = l.take_val() {
                return Ok(v);
            }
        }

        Err(self)
    }
}

impl<T: Send> Future<T> for FutureVal<T> {
    fn receive<F: FnOnce(T) -> () + Send>(self, cb: F) {
        let mut l = self.core.lock();

        if let CompleterWait(strategy) = l.take_completer_wait() {
            match strategy {
                Callback(cb) => {
                    drop(l);
                    cb.call_once((Completer::new(self.core.clone()),));
                    l = self.core.lock();
                }
                Sync => l.condvar.signal(),
            }
        }

        if let Some(v) = l.take_val() {
            drop(l); // Escape the mutex
            cb(v);
            return;
        }

        l.completion = ConsumerWait(Callback(box cb));
    }
}

impl<T: Send> SyncFuture<T> for FutureVal<T> {
    fn take(self) -> T {
        let mut l = self.core.lock();

        if let CompleterWait(strategy) = l.take_completer_wait() {
            match strategy {
                Callback(cb) => {
                    drop(l);
                    cb.call_once((Completer::new(self.core.clone()),));
                    l = self.core.lock();
                }
                Sync => l.condvar.signal(),
            }
        }

        l.completion = ConsumerWait(Sync);

        loop {
            if let Some(v) = l.take_val() {
                return v;
            }

            l.wait(&l.condvar);
        }
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

    // Initializes a new Completer with the given core
    fn new(core: Arc<MutexCell<Core<T>>>) -> Completer<T> {
        Completer { core: core }
    }

    pub fn complete(self, val: T) {
        let mut l = self.core.lock();

        if let ConsumerWait(strategy) = l.take_consumer_wait() {
            match strategy {
                Callback(cb) => {
                    drop(l);
                    cb.call_once((val,));
                    return;
                }
                Sync => {
                    l.put(val);
                    l.condvar.signal();
                }
            }
        } else {
            l.put(val);
        }
    }

    pub fn try_take(self) -> Result<Completer<T>, Completer<T>> {
        if self.is_complete() {
            Ok(self)
        } else {
            Err(self)
        }
    }

    fn is_complete(&self) -> bool {
        let mut l = self.core.lock();
        !l.completion.is_pending()
    }
}

/*
 *
 * ===== Consumer interest future =====
 *
 */

impl<T: Send> Future<Completer<T>> for Completer<T> {
    fn receive<F: FnOnce(Completer<T>) -> () + Send>(self, cb: F) {
        {
            let mut l = self.core.lock();

            if l.completion.is_pending() {
                l.completion = CompleterWait(Callback(box cb));
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
                l.completion = CompleterWait(Sync);

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

    fn take_consumer_wait(&mut self) -> Completion<T> {
        if self.completion.is_consumer_wait() {
            mem::replace(&mut self.completion, Pending)
        } else {
            Pending
        }
    }

    fn take_completer_wait(&mut self) -> Completion<T> {
        if self.completion.is_completer_wait() {
            mem::replace(&mut self.completion, Pending)
        } else {
            Pending
        }
    }
}

// TODO: Rename -> State
enum Completion<T> {
    Pending,
    ConsumerWait(WaitStrategy<T>),
    CompleterWait(WaitStrategy<Completer<T>>),
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
            ConsumerWait(..) => true,
            _ => false,
        }
    }

    fn is_completer_wait(&self) -> bool {
        match *self {
            CompleterWait(..) => true,
            _ => false,
        }
    }
}

enum WaitStrategy<T> {
    Callback(Box<FnOnce<(T,), ()> + Send>),
    Sync,
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
        let (f, c) = future();

        spawn(proc() {
            sleep(Duration::milliseconds(50));
            c.take().complete("zomg");
        });

        assert_eq!("zomg", f.take());
    }

    #[test]
    pub fn test_take_complete_before_consumer_receive() {
        let (f, c) = future();
        let (tx, rx) = channel::<&'static str>();

        spawn(proc() {
            c.take().complete("zomg");
        });

        sleep(Duration::milliseconds(50));
        f.receive(move |:v| tx.send(v));
        assert_eq!(rx.recv(), "zomg");
    }

    #[test]
    pub fn test_take_complete_after_consumer_receive() {
        let (f, c) = future();
        let (tx, rx) = channel::<&'static str>();

        spawn(proc() {
            sleep(Duration::milliseconds(50));
            c.take().complete("zomg");
        });

        f.receive(move |:v| tx.send(v));
        assert_eq!(rx.recv(), "zomg");
    }

    #[test]
    #[ignore]
    pub fn test_completer_receive_when_consumer_cb_set() {
        let (f, c) = future();
        let (tx, rx) = channel::<&'static str>();

        fn waiting(count: uint, c: Completer<&'static str>) {
            println!("WAITING {}", count);
            if count == 5 {
                c.complete("done");
            } else {
                c.receive(move |:c| waiting(count + 1, c));
            }
        }

        waiting(0, c);

        f.receive(move |:v| tx.send(v));
        assert_eq!(rx.recv(), "done");
    }

    #[test]
    pub fn test_completer_take_when_consumer_cb_set() {
        assert!(true);
    }

    #[test]
    pub fn test_completer_receive_when_consumer_waiting() {
        assert!(true);
    }

    #[test]
    pub fn test_completer_take_when_consumer_waiting() {
        assert!(true);
    }
}
