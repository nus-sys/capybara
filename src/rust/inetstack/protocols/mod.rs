// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod arp;
pub mod ethernet2;
pub mod icmpv4;
pub mod ip;
pub mod ipv4;
mod peer;
pub mod tcp;
pub mod udp;

/* #[cfg(feature = "tcp-migration")]
pub mod tcpmig; */

pub use peer::Peer;

pub enum Protocol {
    Tcp,
    Udp,
}
