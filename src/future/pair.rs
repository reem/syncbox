use std::{mem, ptr};
use alloc::heap;
use sync::Park;
use sync::atomic::{AtomicUint, Acquire, Release, Relaxed, fence};
use super::{Future, SyncFuture};

// TODO:
// - Figure out the new function traits
pub fn pair<T>() -> (PairedFuture<T>, Completer<T>) {
    let core = unsafe { mem::transmute(box Core::<T>::new()) };

    let f = PairedFuture { core: core };
    let c = Completer { core: core };

    (f, c)
}

pub struct PairedFuture<T> {
    core: *mut Core<T>,
}

impl<T> PairedFuture<T> {
    #[inline]
    fn core(&mut self) -> &mut Core<T> {
        unsafe { mem::transmute(self.core) }
    }
}

impl<T> Future<T> for PairedFuture<T> {
    fn receive<F: FnOnce<(T,), ()> + Send>(self, cb: F) {
        unimplemented!()
    }
}

impl<T> SyncFuture<T> for PairedFuture<T> {
    fn take(mut self) -> T {
        self.core().take()
    }

    /// Gets the value from the future if it has been completed.
    fn try_take(self) -> Result<T, PairedFuture<T>> {
        unimplemented!()
    }
}

#[unsafe_destructor]
impl<T> Drop for PairedFuture<T> {
    fn drop(&mut self) {
        release(self.core)
    }
}

pub struct Completer<T> {
    core: *mut Core<T>,
}

impl<T: Send> Completer<T> {
    pub fn complete(mut self, value: T) {
        self.core().complete(value)
    }

    pub fn cancel(mut self) {
        self.core().cancel();
    }

    #[inline]
    fn core(&mut self) -> &mut Core<T> {
        unsafe { mem::transmute(self.core) }
    }
}

#[unsafe_destructor]
impl<T> Drop for Completer<T> {
    fn drop(&mut self) {
        release(self.core);
    }
}

fn release<T>(ptr: *mut Core<T>) {
    let core: &mut Core<T> = unsafe { mem::transmute(ptr) };
    // For discusion regarding using Release / Acquire here, see:
    // www.boost.org/doc/libs/1_55_0/doc/html/atomic/usage_examples.html

    // Decrement the ref count
    let old = core.state.fetch_sub(1 << 8, Release);

    // There are still outstanding references
    if (old >> 8) > 1 { return; }

    if state::is_complete(old) {
        // Acqurie any possible changes to the value
        fence(Acquire);

        // Drop the value
        drop(unsafe { core.move_val() });
    }

    // TODO: Free Park if it was allocated

    unsafe {
        heap::deallocate(
            ptr as *mut u8,
            mem::size_of::<Core<T>>(),
            mem::min_align_of::<Core<T>>());
    }
}

// ===== Core =====
//
// The meat of the implementation is here

struct Core<T> {
    // Used to track the state of the future and coordinate memory.
    // Byte layout:
    //   0-3: Future State ID
    //   0-4: wait strategy
    //   8+ : Ref count
    state: AtomicUint,
    value: T,
    wait: Park, // May not actually be Park
}

impl<T> Core<T> {
    fn new() -> Core<T> {
        // The wait strategy defaults to Sync to prevent a mem fence in
        // the sync case. Initializing Park is very lightweight.
        unsafe {
            Core {
                // ref count, future state, wait strategy state
                state: AtomicUint::new(0x0200),
                // completed future value
                value: mem::uninitialized(),
                // how to complete the future
                wait: Park::new(),
            }
        }
    }

    fn take(&mut self) -> T {
        let mut new;

        loop {
            let mut old = self.state.load(Acquire);

            // The future has already been completed, no need to wait
            if state::is_complete(old) {
                new = state::as_consumed(old);

                if old == self.state.compare_and_swap(old, new, Relaxed) {
                    break;
                }
            } else {
                if !state::is_wait_sync(old) {
                    new = state::as_wait_sync(old);

                    if old != self.state.compare_and_swap(old, new, Relaxed) {
                        continue;
                    }
                }

                unsafe { self.parker().park(); }
            }
        }

        unsafe { self.move_val() }
    }

    // Completes the future with the given value. This expects, but does not
    // check, that the current future state is Pending.
    fn complete(&mut self, value: T) {
        // Set the value of the future
        unsafe { ptr::write(&mut self.value as *mut T, value); }

        // Update the state to indicate that the future has been completed
        let prev = self.state.fetch_add(1, Release);

        // Check for a wait strategy. If one is set, the consuming side
        // is waiting for the value.
        if state::is_waiting(prev) {
            // If a callback is used, a fence is going to be required
            unsafe { self.parker().unpark(); }
        }
    }

    fn cancel(&mut self) {
        self.state.fetch_add(2, Relaxed);
    }

    // Take the value leaving the memory in an unsafe state
    unsafe fn move_val(&self) -> T {
        ptr::read(&self.value as *const T)
    }

    fn parker(&self) -> &Park {
        &self.wait
    }

    #[cfg(test)]
    fn ref_count(&self) -> uint {
        let state = self.state.load(Relaxed);
        state >> 8
    }
}

//
//
// ===== State helpers =====
//
//

mod state {
    // Future state
    static STATE_MASK: uint = 0x0f;
    static PENDING:    uint = 0x00;
    static COMPLETE:   uint = 0x01;
    static CANCELED:   uint = 0x02;
    static CONSUMED:   uint = 0x03;

    // Wait state
    static WAIT_MASK: uint = 0xf0;
    static SYNC:      uint = 0x10;

    // Whether or not the future has been completed
    pub fn is_complete(state: uint) -> bool {
        state & STATE_MASK == COMPLETE
    }

    // Whether or not the wait strategy has been set
    pub fn is_waiting(state: uint) -> bool {
        state & WAIT_MASK > 0
    }

    // Track that the future was consumed
    pub fn as_consumed(state: uint) -> uint {
        (state & (!STATE_MASK)) | CONSUMED
    }

    pub fn is_wait_sync(state: uint) -> bool {
        state & WAIT_MASK == SYNC
    }

    // Track that the future is waiting synchronously for the result
    pub fn as_wait_sync(state: uint) -> uint {
        (state & (!WAIT_MASK)) | SYNC
    }
}

#[cfg(test)]
mod test {
    use std::io::timer::sleep;
    use std::time::Duration;
    use super::{pair};
    use {Future, SyncFuture};

    #[test]
    pub fn test_initial_state() {
        let (_f, mut c) = pair::<uint>();

        assert_eq!(c.core().ref_count(), 2);
    }

    #[test]
    pub fn test_complete_before_take() {
        let (f, mut c) = pair();

        spawn(proc() {
            c.complete("zomg");
        });

        sleep(Duration::milliseconds(50));
        assert_eq!(f.take(), "zomg");
    }

    #[test]
    pub fn test_complete_after_take() {
        let (f, mut c) = pair();

        spawn(proc() {
            sleep(Duration::milliseconds(50));
            c.complete("zomg");
        });

        assert_eq!(f.take(), "zomg");
    }

    #[test]
    pub fn test_receive_value() {
        let (f, c) = pair();
        let (tx, rx) = channel::<&'static str>();

        spawn(proc() {
            sleep(Duration::milliseconds(50));
            c.complete("zomg");
        });

        f.receive(move |:v| tx.send(v));
        assert_eq!(rx.recv(), "zomg");
    }
}
