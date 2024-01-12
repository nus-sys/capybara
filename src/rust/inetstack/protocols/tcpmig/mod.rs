pub mod constants;
pub mod segment;
mod peer;
mod active;

use std::cell::Cell;

pub use peer::{TcpMigPeer, log_print, TcpmigReceiveStatus};

use crate::QDesc;

//======================================================================================================================
// Constants
//======================================================================================================================

pub const ETCPMIG: libc::c_int = 199;

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Default)]
pub struct TcpmigPollState {
    migrated_qd: Cell<Option<QDesc>>,
    fast_migrate: Cell<bool>
}

//======================================================================================================================
// Standard Library Trait Implementations
//======================================================================================================================

impl TcpmigPollState {
    #[inline]
    pub fn reset(&self) {
        self.migrated_qd.take();
        self.fast_migrate.take();
    }

    #[inline]
    pub fn take_qd(&self) -> Option<QDesc> {
        self.migrated_qd.take()
    }

    #[inline]
    pub fn set_qd(&self, qd: QDesc) {
        self.migrated_qd.set(Some(qd));
    }

    #[inline]
    pub fn is_fast_migrate_enabled(&self) -> bool {
        self.fast_migrate.get()
    }

    #[inline]
    pub fn enable_fast_migrate(&self) {
        self.fast_migrate.set(true);
    }
}