
pub trait Stream<T> {
    fn each<F: Fn(T) -> () + Send>>(self, cb: F);
}
