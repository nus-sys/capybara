// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::{
    memory::Buffer,
    network::consts::RECEIVE_BATCH_SIZE,
};
use ::arrayvec::ArrayVec;

//==============================================================================
// Exports
//==============================================================================

pub mod config;
pub mod consts;
pub mod types;

//==============================================================================
// Traits
//==============================================================================

/// Packet Buffer
pub trait PacketBuf {
    /// Returns the header size of the target [PacketBuf].
    fn header_size(&self) -> usize;
    /// Writes the header of the target [PacketBuf] into a slice.
    fn write_header(&self, buf: &mut [u8]);
    /// Returns the body size of the target [PacketBuf].
    fn body_size(&self) -> usize;
    /// Consumes and returns the body of the target [PacketBuf].
    fn take_body(&self) -> Option<Buffer>;
}

/// Network Runtime
pub trait NetworkRuntime {
    /// Transmits a single [PacketBuf].
    fn transmit(&self, pkt: Box<dyn PacketBuf>);

    /// Receives a batch of [PacketBuf].
    fn receive(&self) -> ArrayVec<Buffer, RECEIVE_BATCH_SIZE>;

    #[cfg(feature = "tcp-migration")]
    /// Receives a batch of TCPMIG [PacketBuf].
    fn receive_tcpmig(&self) -> ArrayVec<Buffer, RECEIVE_BATCH_SIZE>;
}
