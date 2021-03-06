use syncbox::util::async::*;
use std::sync::mpsc::channel;
use super::{sleep, spawn};

#[test]
pub fn test_joining_two_futures_async() {
    let (f1, c1) = Future::<i32, ()>::pair();
    let (f2, c2) = Future::<i32, ()>::pair();
    let (tx, rx) = channel();

    join((f1, f2)).receive(move |res| {
        tx.send(res.unwrap()).unwrap();
    });

    assert!(rx.try_recv().is_err());
    c1.complete(1);

    assert!(rx.try_recv().is_err());
    c2.complete(2);

    assert_eq!(rx.recv().unwrap(), (1, 2));
}

#[test]
pub fn test_joining_two_futures_sync() {
    let (f1, c1) = Future::<i32, ()>::pair();
    let (f2, c2) = Future::<i32, ()>::pair();

    spawn(move || {
        sleep(50);
        c1.complete(1);
    });

    spawn(move || {
        sleep(75);
        c2.complete(2);
    });

    let vals = join((f1, f2)).await().unwrap();
    assert_eq!(vals, (1, 2));
}

#[test]
pub fn test_lazily_propagates_interest() {
    let (f1, c1) = Future::<i32, ()>::pair();
    let (f2, _c) = Future::<i32, ()>::pair();
    let (tx, rx) = channel();

    c1.receive(move |res| {
        if let Ok(c) = res {
            tx.send("fail").unwrap();
            c.complete(1);
        }
    });

    // Join but don't register a callback
    let _j = join((f1, f2));

    assert!(rx.try_recv().is_err());
}

#[test]
pub fn test_join_errors_on_failure() {
    let (f1, c1) = Future::<i32, ()>::pair();
    let (f2, _c) = Future::<i32, ()>::pair();
    let (tx, rx) = channel();

    join((f1, f2)).receive(move |res| {
        if res.is_err() {
            tx.send("win").unwrap();
        }
    });

    c1.fail(());

    assert_eq!("win", rx.try_recv().unwrap());
}

/*

    Test is blocked by a Rust bug
    https://github.com/rust-lang/rust/issues/21080

#[test]
pub fn test_joining_three_futures_async() {
    let (f1, c1) = Future::<i32, ()>::pair();
    let (f2, c2) = Future::<i32, ()>::pair();
    let (f3, c3) = Future::<i32, ()>::pair();
    let (tx, rx) = channel::<(i32, i32, i32)>();

    join((f1, f2, f3)).receive(move |res: AsyncResult<(i32, i32, i32), ()>| {
        tx.send(res.unwrap()).unwrap();
    });

    assert!(rx.try_recv().is_err());
    c1.complete(1);

    assert!(rx.try_recv().is_err());
    c2.complete(2);

    assert!(rx.try_recv().is_err());
    c3.complete(3);

    assert_eq!(rx.recv().unwrap(), (1, 2, 3));
}
*/
