use time::get_time;
use stdsync::atomic::{
    AtomicUint,
    Relaxed
};
use super::ffi::{
    pthread_mutex_t,
    pthread_cond_t,
    pthread_mutex_lock,
    pthread_mutex_trylock,
    pthread_mutex_unlock,
    pthread_cond_signal,
    pthread_cond_wait,
    pthread_cond_timedwait,
    timespec,
    PTHREAD_MUTEX_INITIALIZER,
    PTHREAD_COND_INITIALIZER,
    EBUSY,
    ETIMEDOUT,
};

static MUTEX_ERR: &'static str = "invalid internal mutex state";
static CONDV_ERR: &'static str = "invalid internal condition variable state";

type LockResult<T> = Result<T, &'static str>;

/// Low level thread parking logic. Only a single thread can call park, but
/// there are no guards for this. Also, there are no memory barriers.
pub struct Park {
    // 1 for unconsumed unpark flag, 0 otherwise
    state: AtomicUint,
    mutex: pthread_mutex_t,
    condvar: pthread_cond_t,
}

// TODO: Reorganize so that asserts don't leave mutexes in an invalid state
impl Park {
    pub fn new() -> Park {
        Park {
            state: AtomicUint::new(0),
            mutex: PTHREAD_MUTEX_INITIALIZER,
            condvar: PTHREAD_COND_INITIALIZER,
        }
    }

    /// Parks the thread until another thread calls unpark or a random wakeup.
    pub unsafe fn park(&self) {
        self.park_ms(0);
    }

    pub unsafe fn park_ms(&self, timeout_ms: uint) {
        let mut old;

        old = self.state.compare_and_swap(1, 0, Relaxed);

        // Fast path, there already is a pending unpark
        if old == 1 {
            return;
        }

        match self.try_lock() {
            // Lock could not be acquired, just give up this loop
            Ok(res) => if res { return; },
            Err(e) => fail!("{}", e)
        }

        // In critical section, check the state again before blocking
        old = self.state.compare_and_swap(1, 0, Relaxed);

        if old == 1 {
            self.unlock().unwrap();
            return;
        }

        // Even if the wait fails, assume that it has been consumed, update the
        // state and carry on.
        let _ = if timeout_ms == 0 {
            self.wait()
        } else {
            self.timed_wait(timeout_ms)
        };

        // Store the new state
        self.state.store(0, Relaxed);

        // Unlock the mutex
        self.unlock().unwrap();
    }

    pub unsafe fn unpark(&self) {
        if self.state.swap(1, Relaxed) == 0 {
            // If there are no threads currently parked, then signaling will
            // have no effect. The lock is to ensure that if the parking thread
            // has entered the critical section, it will have reached the wait
            // point before the signal is fired.
            self.lock().unwrap();
            let _ = self.signal();
            self.unlock().unwrap();
        }
    }

    fn lock(&self) -> LockResult<()> {
        unsafe {
            let res = pthread_mutex_lock(&self.mutex as *const pthread_mutex_t);

            if res < 0 {
                return Err(MUTEX_ERR);
            }

            Ok(())
        }
    }

    fn try_lock(&self) -> LockResult<bool> {
        unsafe {
            let res = pthread_mutex_trylock(&self.mutex as *const pthread_mutex_t);

            match res {
                0 => return Ok(true),
                EBUSY => return Ok(false),
                _ => return Err(MUTEX_ERR),
            }
        }
    }

    fn unlock(&self) -> LockResult<()> {
        unsafe {
            let res = pthread_mutex_unlock(&self.mutex as *const pthread_mutex_t);

            if res < 0 {
                return Err(MUTEX_ERR);
            }

            Ok(())
        }

    }

    fn signal(&self) -> LockResult<()> {
        unsafe {
            let res = pthread_cond_signal(&self.condvar as *const pthread_cond_t);

            if res < 0 {
                return Err(CONDV_ERR);
            }

            Ok(())
        }
    }

    fn wait(&self) -> LockResult<()> {
        unsafe {
            let res = pthread_cond_wait(
                &self.condvar as *const pthread_cond_t,
                &self.mutex as *const pthread_mutex_t);

            if res < 0 {
                return Err(CONDV_ERR);
            }

            Ok(())
        }
    }

    fn timed_wait(&self, ms: uint) -> LockResult<()> {
        let ts = ms_to_abs(ms);

        unsafe {
            let res = pthread_cond_timedwait(
                &self.condvar as *const pthread_cond_t,
                &self.mutex as *const pthread_mutex_t,
                &ts as *const timespec);

            if res == 0 || res == ETIMEDOUT {
                return Ok(());
            }

            Err(CONDV_ERR)
        }
    }
}

static MAX_WAIT: uint = 1_000_000;
static MS_PER_SEC: uint = 1_000;
static NANOS_PER_MS: uint = 1_000_000;
static NANOS_PER_SEC: uint = 1_000_000_000;

fn ms_to_abs(ms: uint) -> timespec {
    use libc::{c_long, time_t};

    let mut ts = get_time();
    let mut sec = ms / MS_PER_SEC;
    let nsec = (ms & MS_PER_SEC) + NANOS_PER_MS;

    if sec > MAX_WAIT {
        sec = MAX_WAIT;
    }

    ts.sec += sec as i64;
    ts.nsec += nsec as i32;

    if ts.nsec >= NANOS_PER_SEC as i32 {
        ts.sec += 1;
        ts.nsec -= NANOS_PER_SEC as i32;
    }

    timespec {
        tv_sec: ts.sec as time_t,
        tv_nsec: ts.nsec as c_long,
    }
}
