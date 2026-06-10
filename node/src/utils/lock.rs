use std::sync::{Mutex, MutexGuard};

//
// Poison-recovering lock helper. The registries guarded by std Mutexes in
// this crate (process handles, cancel flags, ACP state) hold plain data
// whose invariants survive a panic mid-critical-section, so recovering the
// guard is always safe and avoids cascading "lock poisoned" panics across
// unrelated sessions after a single panic.
//

pub trait LockExt<T> {
    fn lock_safe(&self) -> MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn lock_safe(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
