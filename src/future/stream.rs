use super::{Future};

pub trait Stream<T> : Future<Option<(T, Self)>> {
    fn each<F: Fn(T) -> () + Send>(self, cb: F);
}
