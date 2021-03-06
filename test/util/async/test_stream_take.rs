use syncbox::util::async::Future;
use super::nums;

#[test]
pub fn test_stream_take() {
    let stream = nums(0, 10).take(4);
    let vals: Vec<usize> = stream.iter().collect();

    assert_eq!([0, 1, 2, 3].as_slice(), vals.as_slice());
}

/*
 *
 * ===== Stream::take_until(...) =====
 *
 */

#[test]
pub fn test_stream_take_until() {
    let (f, c) = Future::<&'static str, ()>::pair();
    let stream = nums(0, 1_000_000).take_until(f);

    let mut iter = stream.iter();

    for &i in [0, 1, 2, 3, 4].iter() {
        assert_eq!(i, iter.next().unwrap());
    }

    c.complete("done");
    assert!(iter.next().is_none());
}
