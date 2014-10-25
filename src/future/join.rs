use super::Future;
use super::{val, FutureVal};

pub fn join<T1, T2, F1: Future<T1>, F2: Future<T2>>(f1: F1, f2: F2) -> FutureVal<(T1, T2)> {
    unimplemented!()
}

pub fn join_all<T, F: Future<T>>(fs: &[F]) -> FutureVal<Join> {
    unimplemented!()
}

pub struct Join;
