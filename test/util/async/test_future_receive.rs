use syncbox::util::async::*;
use super::{spawn, sleep};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUint};
use std::sync::atomic::Ordering::Relaxed;

#[test]
pub fn test_complete_before_receive() {
    let (f, c) = Future::<&'static str, ()>::pair();
    let (tx, rx) = channel();

    spawn(move || c.complete("zomg"));

    sleep(50);
    f.receive(move |v| tx.send(v.unwrap()).unwrap());
    assert_eq!(rx.recv().unwrap(), "zomg");
}

#[test]
pub fn test_complete_after_receive() {
    let (f, c) = Future::<&'static str, ()>::pair();
    let (tx, rx) = channel();

    spawn(move || {
        sleep(50);
        c.complete("zomg");
    });

    f.receive(move |v| tx.send(v.unwrap()).unwrap());
    assert_eq!(rx.recv().unwrap(), "zomg");
}

#[test]
pub fn test_receive_complete_before_consumer_receive() {
    let (f, c) = Future::<&'static str, ()>::pair();
    let w1 = Arc::new(AtomicBool::new(false));
    let w2 = w1.clone();

    c.receive(move |c| {
        assert!(w2.load(Relaxed));
        c.unwrap().complete("zomg");
    });

    let (tx, rx) = channel();
    w1.store(true, Relaxed);

    f.receive(move |res| {
        assert_eq!("zomg", res.unwrap());
        tx.send("hi2u").unwrap();
    });

    assert_eq!("hi2u", rx.recv().unwrap());
}

#[test]
pub fn test_receive_complete_after_consumer_receive() {
    let (f, c) = Future::<&'static str, ()>::pair();
    let w1 = Arc::new(AtomicBool::new(false));
    let w2 = w1.clone();

    spawn(move || {
        sleep(50);

        c.receive(move |c| {
            assert!(w2.load(Relaxed));
            c.unwrap().complete("zomg");
        });
    });

    let (tx, rx) = channel();
    w1.store(true, Relaxed);

    f.receive(move |res| {
        assert_eq!("zomg", res.unwrap());
        tx.send("hi2u").unwrap();
    });

    assert_eq!("hi2u", rx.recv().unwrap());
}

#[test]
pub fn test_await_complete_before_consumer_receive() {
    let (f, c) = Future::<&'static str, ()>::pair();
    let (tx, rx) = channel();

    spawn(move || {
        debug!("~~~~ Complete::await ~~~~ ");
        c.await().unwrap().complete("zomg");
    });

    sleep(50);

    f.receive(move |res| {
        debug!("~~~~ Future receive ~~~~ | {}", res.is_ok());
        tx.send(res.unwrap()).unwrap();
    });

    assert_eq!(rx.recv().unwrap(), "zomg");
}

#[test]
pub fn test_await_complete_after_consumer_receive() {
    let (f, c) = Future::<&'static str, ()>::pair();
    let (tx, rx) = channel();

    spawn(move || {
        sleep(50);
        debug!("complete await");
        c.await().unwrap().complete("zomg");
    });

    f.receive(move |res| {
        debug!("future receive");
        tx.send(res.unwrap()).unwrap()
    });

    assert_eq!(rx.recv().unwrap(), "zomg");
}

#[test]
pub fn test_producer_receive_when_consumer_cb_set() {
    let (f, c) = Future::<&'static str, ()>::pair();
    let (tx, rx) = channel();
    let depth = Arc::new(AtomicUint::new(0));

    waiting(0, depth, c);

    f.receive(move |res| {
        tx.send(res.unwrap()).unwrap()
    });

    assert_eq!(rx.recv().unwrap(), "done");
}

#[test]
pub fn test_producer_receive_when_consumer_waiting() {
    let (f, c) = Future::<&'static str, ()>::pair();
    let depth = Arc::new(AtomicUint::new(0));

    waiting(0, depth, c);

    assert_eq!(f.await().unwrap(), "done");
}

fn waiting(count: uint, d: Arc<AtomicUint>, c: Complete<&'static str, ()>) {
    // Assert that the callback is not invoked recursively
    assert_eq!(0, d.fetch_add(1, Relaxed));

    if count == 5 {
        c.complete("done");
    } else {
        let d2 = d.clone();
        c.receive(move |c| waiting(count + 1, d2, c.unwrap()));
    }

    d.fetch_sub(1, Relaxed);
}

#[test]
pub fn test_producer_await_when_consumer_receive() {
    let (f, c) = Future::<&'static str, ()>::pair();
    let (tx, rx) = channel();

    spawn(move || {
        c.await().unwrap()
            .await().unwrap()
            .await().unwrap().complete("zomg");
    });

    sleep(50);

    f.receive(move |res| {
        tx.send(res.unwrap()).unwrap()
    });

    assert_eq!(rx.recv().unwrap(), "zomg");
}

#[test]
pub fn test_canceling_future_before_producer_receive() {
    let (f, c) = Future::<uint, ()>::pair();
    let (tx, rx) = channel();

    drop(f);

    c.receive(move |c| {
        // TODO: Clean this up https://github.com/rust-lang/rfcs/pull/565#issuecomment-71090271
        match c {
            Err(e) => assert!(e.is_cancellation()),
            _ => panic!("nope"),
        }

        tx.send("done").unwrap();
    });

    assert_eq!(rx.recv().unwrap(), "done");
}

#[test]
pub fn test_canceling_future_before_producer_await() {
    let (f, c) = Future::<uint, ()>::pair();

    drop(f);

    assert!(c.await().is_err());
}

#[test]
pub fn test_canceling_future_after_producer_receive() {
    let (f, c) = Future::<uint, ()>::pair();
    let (tx, rx) = channel();

    c.receive(move |c| {
        assert!(c.is_err());
        tx.send("done").unwrap();
    });

    drop(f);
    assert_eq!(rx.recv().unwrap(), "done");
}

#[test]
pub fn test_canceling_future_after_producer_await() {
    let (f, c) = Future::<uint, ()>::pair();
    let (tx, rx) = channel();

    spawn(move || {
        assert!(c.await().is_err());
        tx.send("done").unwrap();
    });

    sleep(50);
    drop(f);

    assert_eq!(rx.recv().unwrap(), "done");
}

#[test]
pub fn test_canceling_producer_then_receive() {
    let (f, c) = Future::<uint, ()>::pair();
    let (tx, rx) = channel();

    drop(c);

    f.receive(move |res| {
        assert!(res.is_err());
        tx.send("done").unwrap();
    });

    assert_eq!(rx.recv().unwrap(), "done");
}

#[test]
pub fn test_producer_fail_consumer_receive() {
    let (f, c) = Future::<uint, &'static str>::pair();

    spawn(move || {
        sleep(50);
        c.fail("nope");
    });

    let err = f.await().unwrap_err();
    assert!(err.is_execution_error());
    assert_eq!(err.unwrap(), "nope");
}

#[test]
pub fn test_panic_cancels_future() {
}
