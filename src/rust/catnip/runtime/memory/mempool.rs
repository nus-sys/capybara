// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::{
    fail::Fail,
    libdpdk::{
        rte_mbuf,
        rte_mempool,
        rte_pktmbuf_alloc,
        rte_pktmbuf_clone,
        rte_pktmbuf_free,
        rte_pktmbuf_pool_create,
        rte_socket_id,
        rte_eal_process_type,
        rte_mempool_lookup,
    },
};
use ::std::ffi::CString;

//==============================================================================
// Structures
//==============================================================================

/// DPDK Memory Pool
#[derive(Debug)]
pub struct MemoryPool {
    /// Underlying memory pool.
    pool: *mut rte_mempool,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associated functions for memory pool.
impl MemoryPool {
    /// Creates a new memory pool.
    pub fn new(name: CString, data_room_size: usize, pool_size: usize, cache_size: usize) -> Result<Self, Fail> {
        let pool: *mut rte_mempool;
        let proc_type = unsafe {rte_eal_process_type()};
        match proc_type {
            0 => {
                println!("Running as primary process");
                unsafe{ println!("name: {:?}\npool_size: {:?}\ncache_size: {:?}\ndata_room_size: {:?}\nrte_socket_id: {:?}\n", 
                name, pool_size, cache_size, data_room_size, rte_socket_id()) };
                pool = unsafe {
                    rte_pktmbuf_pool_create(
                        name.as_ptr(),
                        pool_size as u32,
                        cache_size as u32,
                        0,
                        data_room_size as u16,
                        rte_socket_id() as i32,
                    )
                    // ... do something for primary process
                }
            }
            1 => {
                println!("Running as secondary process");
                pool = unsafe {rte_mempool_lookup(name.as_ptr())}
                // ... do something for secondary process
            },
            _ => panic!("Unknown process type"),
        }
            

        // Failed to create memory pool.
        if pool.is_null() {
            return Err(Fail::new(libc::EAGAIN, "failed to create memory pool"));
        }

        Ok(Self { pool })
    }

    /// Gets a raw pointer to the underlying memory pool.
    pub fn into_raw(&self) -> *mut rte_mempool {
        self.pool
    }

    /// Allocates a mbuf in the target memory pool.
    pub fn alloc_mbuf(&self, size: Option<usize>) -> Result<*mut rte_mbuf, Fail> {
        // TODO: Drop the following warning once DPDK memory management is more stable.
        warn!("allocating mbuf from DPDK pool");

        // Allocate mbuf.
        let mut mbuf_ptr: *mut rte_mbuf = unsafe { rte_pktmbuf_alloc(self.pool) };
        if mbuf_ptr.is_null() {
            return Err(Fail::new(libc::ENOMEM, "cannot allocate more mbufs"));
        }

        // Fill out some fields of the underlying mbuf.
        unsafe {
            let mut num_bytes: u16 = (*mbuf_ptr).buf_len - (*mbuf_ptr).data_off;

            if let Some(size) = size {
                // Check if allocated buffer is big enough.
                if (size as u16) > num_bytes {
                    // Allocated buffer is not big enough, rollback allocation.
                    rte_pktmbuf_free(mbuf_ptr);
                    return Err(Fail::new(libc::EFAULT, "cannot allocate a mbuf this big"));
                }
                num_bytes = size as u16;
            }
            (*mbuf_ptr).data_len = num_bytes;
            (*mbuf_ptr).pkt_len = num_bytes as u32;
        }

        Ok(mbuf_ptr)
    }

    /// Releases a mbuf in the target memory pool.
    pub fn free_mbuf(mbuf_ptr: *mut rte_mbuf) {
        unsafe {
            rte_pktmbuf_free(mbuf_ptr);
        }
    }

    /// Clones a mbuf into a memory pool.
    pub fn clone_mbuf(mbuf_ptr: *mut rte_mbuf) -> Result<*mut rte_mbuf, Fail> {
        unsafe {
            let mempool_ptr: *mut rte_mempool = (*mbuf_ptr).pool;
            let mbuf_ptr_clone: *mut rte_mbuf = rte_pktmbuf_clone(mbuf_ptr, mempool_ptr);
            if mbuf_ptr_clone.is_null() {
                return Err(Fail::new(libc::EINVAL, "cannot clone mbuf"));
            }

            Ok(mbuf_ptr_clone)
        }
    }
}
