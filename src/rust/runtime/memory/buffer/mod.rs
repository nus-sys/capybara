// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod databuffer;
#[cfg(feature = "libdpdk")]
mod dpdkbuffer;

//==============================================================================
// Imports
//==============================================================================

use ::core::ops::{
    Deref,
    DerefMut,
};
use ::std::fmt::Debug;

//==============================================================================
// Exports
//==============================================================================

pub use self::databuffer::DataBuffer;
#[cfg(feature = "libdpdk")]
pub use self::dpdkbuffer::DPDKBuffer;

//==============================================================================
// Enumerations
//==============================================================================

#[derive(Clone, Debug)]
pub enum Buffer {
    Heap(DataBuffer),
    #[cfg(feature = "libdpdk")]
    DPDK(DPDKBuffer),
}

//==============================================================================
// Associated Functions
//==============================================================================

impl Buffer {
    /// Removes bytes from the front of the target data buffer.
    pub fn adjust(&mut self, nbytes: usize) {
        match self {
            Buffer::Heap(dbuf) => dbuf.adjust(nbytes),
            #[cfg(feature = "libdpdk")]
            Buffer::DPDK(mbuf) => mbuf.adjust(nbytes),
        }
    }

    /// Removes bytes from the end of the target data buffer.
    pub fn trim(&mut self, nbytes: usize) {
        match self {
            Buffer::Heap(dbuf) => dbuf.trim(nbytes),
            #[cfg(feature = "libdpdk")]
            Buffer::DPDK(mbuf) => mbuf.trim(nbytes),
        }
    }
}

//==============================================================================
// Standard-Library Trait Implementations
//==============================================================================

/// De-Reference Trait Implementation for Data Buffers
impl Deref for Buffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            Buffer::Heap(dbuf) => dbuf.deref(),
            #[cfg(feature = "libdpdk")]
            Buffer::DPDK(mbuf) => mbuf.deref(),
        }
    }
}

/// Mutable De-Reference Trait Implementation for Data Buffers
impl DerefMut for Buffer {
    fn deref_mut(&mut self) -> &mut [u8] {
        match self {
            Buffer::Heap(dbuf) => dbuf.deref_mut(),
            #[cfg(feature = "libdpdk")]
            Buffer::DPDK(mbuf) => mbuf.deref_mut(),
        }
    }
}

impl PartialEq for Buffer {
    fn eq(&self, other: &Self) -> bool {
        self.deref() == other.deref()
    }
}

impl Eq for Buffer {}

//==============================================================================
// Crate Serde Trait Implementations
//==============================================================================

impl serde::Serialize for Buffer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> 
    where S: 
        serde::Serializer,
    {
        match self {
            Buffer::Heap(dbuf) => dbuf.serialize(serializer),
            #[cfg(feature = "libdpdk")]
            Buffer::DPDK(mbuf) => DataBuffer::from_slice(&mbuf).serialize(serializer),
        }
    }
}

impl<'de> serde::Deserialize<'de> for Buffer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D:
        serde::Deserializer<'de>,
    {
        // DPDKBuffers are never serialized as DPDKBuffers.
        Ok(Self::Heap(DataBuffer::deserialize(deserializer)?))
    }
}