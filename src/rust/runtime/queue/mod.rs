// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod qdesc;
mod qresult;
mod qtoken;
mod qtype;

//==============================================================================
// Imports
//==============================================================================

use ::slab::Slab;

//==============================================================================
// Imports
//==============================================================================

pub use self::{
    qdesc::QDesc,
    qresult::QResult,
    qtoken::QToken,
    qtype::QType,
};

//==============================================================================
// Structures
//==============================================================================

/// IO Queue Table
pub struct IoQueueTable {
    table: Slab<u32>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for IO Queue Tables
impl IoQueueTable {
    /// Offset for I/O queue descriptors.
    ///
    /// When Demikernel is interposing system calls of the underlying OS
    /// this offset enables us to distinguish I/O queue descriptors from
    /// file descriptors.
    ///
    /// NOTE: This is intentionally set to be half of FD_SETSIZE (1024) in Linux.
    const BASE_QD: usize = 500;

    /// Creates an IO queue table.
    pub fn new() -> Self {
        Self { table: Slab::new() }
    }

    /// Allocates a new entry in the target [IoQueueTable].
    pub fn alloc(&mut self, qtype: u32) -> QDesc {
        let index: usize = self.table.insert(qtype);

        // Ensure that the allocation would yield to a safe conversion between usize to u32.
        // Note: This imposes a limit on the number of open queue descriptors in u32::MAX.
        // sgd: assuming `u32::MAX` has been rewritten to `usize::MAX` during a type refactor
        // so the assertion is not meaningful anymore
        // assert!(
        //     index + Self::BASE_QD <= usize::MAX,
        //     "I/O descriptors table overflow"
        // );

        QDesc::from(index + Self::BASE_QD)
    }

    /// Gets the file associated with an IO queue descriptor.
    pub fn get(&self, qd: QDesc) -> Option<u32> {
        if Into::<usize>::into(qd) < Self::BASE_QD {
            None
        } else {
            let rawqd: usize = Into::<usize>::into(qd) - Self::BASE_QD;
            if !self.table.contains(rawqd) {
                return None;
            }

            self.table.get(rawqd).cloned() 
        }        
    }

    /// Releases an entry in the target [IoQueueTable].
    pub fn free(&mut self, qd: QDesc) -> Option<u32> {
        if !self.table.contains(qd.into()) {
            return None;
        }

        Some(self.table.remove(qd.into()))
    }
}

//==============================================================================
// Unit Tests
//==============================================================================

#[cfg(test)]
mod tests {
    use super::IoQueueTable;
    use crate::{
        QDesc,
        QType,
    };
    use ::test::{
        black_box,
        Bencher,
    };

    #[bench]
    fn bench_alloc_free(b: &mut Bencher) {
        let mut ioqueue_table: IoQueueTable = IoQueueTable::new();

        b.iter(|| {
            let qd: QDesc = ioqueue_table.alloc(QType::TcpSocket.into());
            black_box(qd);
            let qtype: Option<u32> = ioqueue_table.free(qd);
            black_box(qtype);
        });
    }
}
