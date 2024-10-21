// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::mempool::MemoryPool;
use crate::{
    capy_log, inetstack::protocols::{
        ethernet2::ETHERNET2_HEADER_SIZE,
        ipv4::IPV4_HEADER_DEFAULT_SIZE,
        tcp::MAX_TCP_HEADER_SIZE,
    }, runtime::{
        fail::Fail,
        libdpdk::{
            rte_eal_process_type, rte_mbuf, rte_mempool
        },
        memory::{
            Buffer,
            DPDKBuffer,
            DataBuffer,
        },
        types::{
            demi_sgarray_t,
            demi_sgaseg_t,
        },
    }
};
use ::anyhow::Error;
use ::libc::c_void;
use ::std::{
    ffi::CString,
    mem,
    ptr,
    rc::Rc,
    slice,
};

//==============================================================================
// Exports
//==============================================================================

pub use super::config::MemoryConfig;

//==============================================================================
// Structures
//==============================================================================

// TODO: Drop this structure.
#[derive(Debug)]
pub struct Inner {
    config: MemoryConfig,

    // Used by networking stack for protocol headers + inline bodies. These buffers are only used
    // internally within the network stack.
    header_pool: Rc<MemoryPool>,

    // Large body pool for buffers given to the application for zero-copy.
    body_pool: Rc<MemoryPool>,
}

/// Memory Manager
#[derive(Clone, Debug)]
pub struct MemoryManager {
    inner: Rc<Inner>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associated Functions for Memory Managers
impl MemoryManager {
    /// Instantiates a memory manager.
    pub fn new(max_body_size: usize) -> Result<Self, Error> {
        let memory_config: MemoryConfig = MemoryConfig::new(None, None, Some(max_body_size), None, None);

        Ok(Self {
            inner: Rc::new(Inner::new(memory_config)?),
        })
    }

    /// Converts a runtime buffer into a scatter-gather array.
    pub fn into_sgarray(&self, buf: Buffer) -> Result<demi_sgarray_t, Fail> {
        let (mbuf_ptr, sgaseg): (*mut rte_mbuf, demi_sgaseg_t) = match buf {
            // Heap-managed buffer.
            Buffer::Heap(dbuf) => {
                let len: usize = dbuf.len();
                let offset: usize = dbuf.offset();
                let dbuf_ptr: *const [u8] = DataBuffer::into_raw(Clone::clone(&dbuf))?;
                let dbuf_ptr: *mut c_void = unsafe { (dbuf_ptr as *const u8).add(offset) } as *mut c_void;
                (
                    ptr::null_mut(),
                    demi_sgaseg_t {
                        sgaseg_buf: dbuf_ptr,
                        sgaseg_len: len as u32,
                    },
                )
            },
            // DPDK-managed buffer.
            Buffer::DPDK(mbuf) => {
                let mbuf_ptr: *mut rte_mbuf = mbuf.get_ptr();
                let sgaseg: demi_sgaseg_t = demi_sgaseg_t {
                    sgaseg_buf: mbuf.data_ptr() as *mut c_void,
                    sgaseg_len: mbuf.len() as u32,
                };
                mem::forget(mbuf);
                (mbuf_ptr, sgaseg)
            },
        };

        // TODO: Drop the sga_addr field in the scatter-gather array.
        Ok(demi_sgarray_t {
            sga_buf: mbuf_ptr as *mut c_void,
            sga_numsegs: 1,
            sga_segs: [sgaseg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }

    /// Allocates a header mbuf.
    /// TODO: Review the need of this function after we are done with the refactor of the DPDK runtime.
    pub fn alloc_header_mbuf(&self) -> Result<DPDKBuffer, Fail> {
        let mbuf_ptr: *mut rte_mbuf = self.inner.header_pool.alloc_mbuf(None)?;
        Ok(DPDKBuffer::new(mbuf_ptr))
    }

    /// Allocates a body mbuf.
    /// TODO: Review the need of this function after we are done with the refactor of the DPDK runtime.
    pub fn alloc_body_mbuf(&self) -> Result<DPDKBuffer, Fail> {
        let mbuf_ptr: *mut rte_mbuf = self.inner.body_pool.alloc_mbuf(None)?;
        Ok(DPDKBuffer::new(mbuf_ptr))
    }

    /// Allocates a scatter-gather array.
    pub fn alloc_sgarray(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        capy_log!("alloc_sgarray, size: {}", size);
        capy_log!("Inline Body Size: {}", self.inner.config.get_inline_body_size());
        capy_log!("Max Body Size: {}", self.inner.config.get_max_body_size());
        // Allocate underlying buffer.
        let (mbuf_ptr, sgaseg): (*mut rte_mbuf, demi_sgaseg_t) =
            if size > self.inner.config.get_inline_body_size() && size <= self.inner.config.get_max_body_size() {
                capy_log!("[1]");
                // Allocate a DPDK-managed buffer.
                let mbuf_ptr: *mut rte_mbuf = self.inner.body_pool.alloc_mbuf(Some(size))?;
                capy_log!("[1-1]");
                // Adjust various fields in the mbuf and create a scatter-gather segment out of it.
                unsafe {
                    capy_log!("[1-2]");
                    let buf_ptr: *mut u8 = (*mbuf_ptr).buf_addr as *mut u8;
                    let data_ptr: *mut u8 = buf_ptr.offset((*mbuf_ptr).data_off as isize);
                    capy_log!("[1-3]");
                    (
                        mbuf_ptr,
                        demi_sgaseg_t {
                            sgaseg_buf: data_ptr as *mut c_void,
                            sgaseg_len: size as u32,
                        },
                    )
                }
            } else {
                capy_log!("[2]");
                // Allocate a heap-managed buffer.
                let dbuf: DataBuffer = DataBuffer::new(size)?;
                let dbuf_ptr: *const [u8] = DataBuffer::into_raw(dbuf)?;
                (
                    ptr::null_mut(),
                    demi_sgaseg_t {
                        sgaseg_buf: dbuf_ptr as *mut c_void,
                        sgaseg_len: size as u32,
                    },
                )
            };
            capy_log!("RETURN");
        // TODO: Drop the sga_addr field in the scatter-gather array.
        Ok(demi_sgarray_t {
            sga_buf: mbuf_ptr as *mut c_void,
            sga_numsegs: 1,
            sga_segs: [sgaseg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }

    /// Releases a scatter-gather array.
    pub fn free_sgarray(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        // Check arguments.
        // TODO: Drop this check once we support scatter-gather arrays with multiple segments.
        if sga.sga_numsegs != 1 {
            return Err(Fail::new(libc::EINVAL, "scatter-gather array with invalid size"));
        }

        // Release underlying buffer.
        if !sga.sga_buf.is_null() {
            // Release DPDK-managed buffer.
            let mbuf_ptr: *mut rte_mbuf = sga.sga_buf as *mut rte_mbuf;
            MemoryPool::free_mbuf(mbuf_ptr);
        } else {
            // Release heap-managed buffer.
            let sgaseg: demi_sgaseg_t = sga.sga_segs[0];
            let (data_ptr, length): (*mut u8, usize) = (sgaseg.sgaseg_buf as *mut u8, sgaseg.sgaseg_len as usize);

            // Convert back to a heap buffer and drop allocation.
            DataBuffer::from_raw_parts(data_ptr, length)?;
        }

        Ok(())
    }

    /// Clones a scatter-gather array.
    pub fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<Buffer, Fail> {
        // Check arguments.
        // TODO: Drop this check once we support scatter-gather arrays with multiple segments.
        if sga.sga_numsegs != 1 {
            return Err(Fail::new(libc::EINVAL, "scatter-gather array with invalid size"));
        }

        let sgaseg: demi_sgaseg_t = sga.sga_segs[0];
        let (ptr, len): (*mut c_void, usize) = (sgaseg.sgaseg_buf, sgaseg.sgaseg_len as usize);

        // Clone underlying buffer.
        let buf: Buffer = if !sga.sga_buf.is_null() {
            // Clone DPDK-managed buffer.
            let mbuf_ptr: *mut rte_mbuf = sga.sga_buf as *mut rte_mbuf;
            let body_clone: *mut rte_mbuf = match MemoryPool::clone_mbuf(mbuf_ptr) {
                Ok(mbuf_ptr) => mbuf_ptr,
                Err(e) => panic!("failed to clone mbuf: {:?}", e.cause),
            };
            let mut mbuf: DPDKBuffer = DPDKBuffer::new(body_clone);
            // Adjust buffer length.
            // TODO: Replace the following method for computing the length of a mbuf once we have a proper DPDKBuffer abstraction.
            let orig_len: usize = unsafe { ((*mbuf_ptr).buf_len - (*mbuf_ptr).data_off).into() };
            let trim: usize = orig_len - len;
            capy_log!("Original length: {}", orig_len);
            capy_log!("Trim length: {}", trim);
            capy_log!("mbuf length: {}", mbuf.len());
            // mbuf.trim(trim);
            Buffer::DPDK(mbuf)
        } else {
            // Clone heap-managed buffer.
            let seg_slice: &[u8] = unsafe { slice::from_raw_parts(ptr as *const u8, len) };
            let buf: DataBuffer = DataBuffer::from_slice(seg_slice);
            Buffer::Heap(buf)
        };

        Ok(buf)
    }

    /// Returns a raw pointer to the underlying body pool.
    /// TODO: Review the need of this function after we are done with the refactor of the DPDK runtime.
    pub fn body_pool(&self) -> *mut rte_mempool {
        self.inner.body_pool.into_raw()
    }
}

/// Associated Functions for Memory Managers
impl Inner {
    fn new(config: MemoryConfig) -> Result<Self, Error> {
        // TODO: The following computation for header size is bad. It should be fixed to maximum possible size.
        let header_size: usize = ETHERNET2_HEADER_SIZE + IPV4_HEADER_DEFAULT_SIZE + MAX_TCP_HEADER_SIZE;
        let header_mbuf_size: usize = header_size + config.get_inline_body_size();

        // Create memory pool for holding packet headers.
        let header_pool: MemoryPool = MemoryPool::new(
            CString::new("header_pool")?,
            header_mbuf_size,
            config.get_header_pool_size(),
            config.get_cache_size(),
        )?;

        // Create memory pool for holding packet bodies.
        let body_pool: MemoryPool = MemoryPool::new(
            CString::new("body_pool")?,
            config.get_max_body_size(),
            config.get_body_pool_size(),
            config.get_cache_size(),
        )?;

        Ok(Self {
            config,
            header_pool: Rc::new(header_pool),
            body_pool: Rc::new(body_pool),
        })
    }
}
