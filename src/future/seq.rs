//! Implementes an stream of values with a fixed buffer size. By
//! default, the buffer size is 1.

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

pub type Next<T> = Option<(T, Seq<T>)>;

pub struct Seq<T> {
    core: Arc<MutexCell<Core<T>>>,
}

impl<T: Send> Future<Next<T>> for Seq<T> {
    fn receive<F: FnOnce(Next<T>) -> () + Send>(self, cb: F) {
        let mut head;

        // Scope required for borrow checker
        {
            // Acquire the mutex
            let mut l = self.core.lock();

            // The interaction with a waiting producer depends on the
            // current state of the stream: whether there currently is a
            // value or not.
            //
            // A value is present:
            //
            //   The stream is currently full, the value is consumed
            //   before notifying the producer of a state change.
            //
            // No value is present:
            //
            //   The stream is "Ready" (can buffer a value). Register
            //   the consumer callback then notify the producer of the
            //   state change.

            // Try taking the head value
            if let Some(h) = l.take_head() {
                // If there is a value, save it for once the lock scope
                // is escaped.
                head = h;
            } else {
                // No head yet, indicate interest by registering the
                // callback.
                l.state = ConsumerCb(box cb);
                return;
            }
        }

        // The head of the Seq has been realized, invoke the callback
        // with it.
        match head {
            More(v) => cb(Some((v, self))),
            Done => cb(None),
        }
    }
}

impl<T: Send> SyncFuture<Next<T>> for Seq<T> {
    fn take(self) -> Next<T> {
        let mut head;

        // Satisfy the borrow checker
        {
            // Acquire the mutex
            let mut l = self.core.lock();
            l.state = ConsumerWait;

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
    fn try_take(self) -> Result<Next<T>, Seq<T>> {
        unimplemented!()
    }
}

/// The producing half of Seq. Each item sent to Seq realizes the
/// current head.
///
/// SeqProducer is also a stream of consumer state changes. This can be
/// used by the producing code to watch whether or not the consumer has
/// registered an interest for the next element of the Seq.
pub struct SeqProducer<T> {
    core: Arc<MutexCell<Core<T>>>,
}

impl<T: Send> SeqProducer<T> {
    pub fn send(self, val: T) {
        let mut l = self.core.lock();

        if let ConsumerCb(cb) = l.take_callback() {
            drop(l);

            // The rest of the stream
            let rest = Seq { core: self.core.clone() };
            cb.call_once((Some((val, rest)),));
            return;
        }

        l.put(val);

        if l.state.is_consumer_wait() {
            l.condvar.signal();
        }
    }
}

/// The possible states for a consumer to be in.
#[deriving(Show, PartialEq, Eq)]
pub enum ConsumerState {
    /// The Seq can buffer another value, but the consumer has not
    /// indicated any interest yet.
    Ready,
    /// The consumer has expressed interest in the next value.
    Waiting,
    /// The stream cannot currently accept any further values.
    Full,
}

pub type NextConsumerState<T> = Option<(ConsumerState, SeqProducer<T>)>;

impl<T: Send> Future<NextConsumerState<T>> for SeqProducer<T> {
    fn receive<F: FnOnce(NextConsumerState<T>) -> () + Send>(self, cb: F) {
        let mut curr;

        {
            let mut l = self.core.lock();

            // Get the current consumer state
            curr = Some(l.consumer_state());

            // If the state is identical to the last observed state,
            // then it is not interesting. Save off the callback for
            // later invocation.
            if curr == l.last_observed {
                l.state = ProducerCb(box cb);
                return;
            }

            // The state has changed since last observation, record the
            // new one.
            l.last_observed = curr;
        }

        // Invoke the callback with the new state
        cb(Some((curr.unwrap(), self)));
    }
}

impl<T: Send> SyncFuture<NextConsumerState<T>> for SeqProducer<T> {
    fn take(self) -> NextConsumerState<T> {
        unimplemented!();
    }

    fn try_take(self) -> Result<NextConsumerState<T>, SeqProducer<T>> {
        unimplemented!()
    }
}

// Seq state shared between consumer and producer; guarded by a mutex.
// This is implemented with a mutex & condvar fo rnow, but hopefully
// Rust will add support for thread park / unpark.
struct Core<T> {
    head: Option<Head<T>>,
    condvar: CondVar,
    state: State<T>,
    // The last consumer state observed by the producer is tracked in
    // order to maintain the necessary semantics.
    last_observed: ConsumerState,
}

impl<T: Send> Core<T> {
    fn new() -> Core<T> {
        Core {
            head: None,
            condvar: CondVar::new(),
            state: Pending,
            last_observed: Full,
        }
    }

    fn put(&mut self, val: T) {
        assert!(self.head.is_none(), "stream not ready for next value");
        self.head = Some(More(val));
    }

    fn take_head(&mut self) -> Option<Head<T>> {
        mem::replace(&mut self.head, None)
    }

    fn take_callback(&mut self) -> State<T> {
        if self.state.is_callback() {
            mem::replace(&mut self.state, Pending)
        } else {
            Pending
        }
    }

    fn consumer_state(&self) -> ConsumerState {
        match self.state {
            Pending => Ready,
            ConsumerWait | ConsumerCb(..) => Waiting,
            ProducerWait | ProducerCb(..) => {
                if self.head.is_some() {
                    Full
                } else {
                    Ready
                }
            }
        }
    }
}

enum Head<T> {
    More(T),
    Done,
}

enum State<T> {
    Pending,
    ConsumerWait,
    ProducerWait,
    ConsumerCb(Box<FnOnce<(Next<T>,), ()> + Send>),
    ProducerCb(Box<FnOnce<(NextConsumerState<T>,), ()> + Send>),
}

impl<T: Send> State<T> {
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

    fn is_callback(&self) -> bool {
        match *self {
            ConsumerCb(..) => true,
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

    #[test]
    pub fn test_producer_receive_when_consumer_cb_set() {
        // The consumer is waiting for a value, the producer is
        // notified, but instead of producing a value, the producer
        // waits for another state change.
        assert!(true);
    }

    #[test]
    pub fn test_producer_take_when_consumer_cb_set() {
        // Same as above, but with take instead of receive
        assert!(true);
    }
}
