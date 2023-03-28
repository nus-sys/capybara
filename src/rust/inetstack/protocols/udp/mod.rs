// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! # User Datagram Protocol
//!
//! # References
//!
//! - https://datatracker.ietf.org/doc/html/rfc768.

mod datagram;
mod futures;
mod peer;
mod queue;

#[cfg(test)]
mod tests;

//==============================================================================
// Exports
//==============================================================================

pub use self::{
    datagram::{
        UdpHeader,
        UdpDatagram,
        UDP_HEADER_SIZE,
    },
    futures::{
        UdpOperation,
        UdpPopFuture,
    },
    peer::UdpPeer,
};
