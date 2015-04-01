use std::thunk::Thunk;

pub trait Run {
    /// Runs the task on the underlying executor.
    ///
    /// This uses a `Thunk` rather than a generic `FnOnce` so that
    /// `Run` is object safe.
    fn run(&self, task: Thunk<'static>);
}

#[test]
fn test_run_is_object_safe() {
    struct X;

    impl Run for X {
        fn run(&self, _: Thunk) {}
    }

    let _x: &Run = &X;
}

