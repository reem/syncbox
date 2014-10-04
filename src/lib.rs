#![crate_name = "pythia"]

// Enable some features
#![feature(phase)]
#![feature(unboxed_closures)]
#![feature(overloaded_calls)]
#![feature(unsafe_destructor)]

extern crate alloc;
extern crate libc;
extern crate time;
extern crate "sync" as stdsync;

pub use future::{Future, SyncFuture};
pub use sync::Park;

pub mod future;
mod sync;
