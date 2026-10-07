//! State shared between the tracking thread and the IPC commands.
//!
//! Locks are only ever held for a store call or a field copy — never across a probe sample
//! or a view computation. A poisoned lock (a panic elsewhere) still yields its data: every
//! write here is a single assignment or a single SQLite statement, so nothing is half-done.

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use timewent_core::Config;
use timewent_store::Store;

/// `Store` is `Send` but not `Sync`.
pub type SharedStore = Arc<Mutex<Store>>;
pub type SharedConfig = Arc<RwLock<Config>>;
/// Successful appends so far: a cheap "has anything changed" stamp for derived state.
pub type SharedCounter = Arc<AtomicU64>;

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn read<T>(l: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(PoisonError::into_inner)
}

pub fn write<T>(l: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(PoisonError::into_inner)
}
