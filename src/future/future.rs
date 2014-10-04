pub trait Future<T> {
    fn receive<F: FnOnce<(T,), ()> + Send>(self, cb: F);
}
