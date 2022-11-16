//==============================================================================
// Imports
//==============================================================================

use std::{
    collections::{
        HashMap,
        VecDeque
    },
    net::SocketAddrV4
};

use crate::{
    inetstack::protocols::{
        ipv4::Ipv4Header,
        tcp::segment::TcpHeader
    },
    runtime::memory::Buffer
};

//==============================================================================
// Structures
//==============================================================================

pub struct TcpMigrationData {
    // Migrate-in data

    /// Original origins for established migrated-in connections.
    /// 
    /// (local, remote) -> origin
    pub origins: HashMap<(SocketAddrV4, SocketAddrV4), SocketAddrV4>,

    /// For not yet migrated in connections.
    /// 
    /// remote -> queue
    /// 
    /// TODO: (local, remote) -> queue
    pub recv_queues: HashMap<SocketAddrV4, VecDeque<(Ipv4Header, TcpHeader, Buffer)>>,

    
    // Migrate-out data

    /// For connections that need to wait for a PREPARE_MIGRATION_ACK.
    /// 
    /// (origin, target, remote) -> has_received ACK
    pub ack_pending: HashMap<(SocketAddrV4, SocketAddrV4, SocketAddrV4), bool>,

    /// Migration-locked connections.
    pub locked: HashMap<(SocketAddrV4, SocketAddrV4), bool>,
}

//==============================================================================
// Associated FUnctions
//==============================================================================

impl TcpMigrationData {
    pub fn new() -> Self {
        Self {
            origins: HashMap::new(),
            recv_queues: HashMap::new(),
            ack_pending: HashMap::new(),
            locked: HashMap::new(),
        }
    }
}