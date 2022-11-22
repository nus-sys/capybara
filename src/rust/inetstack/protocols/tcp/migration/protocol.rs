//==============================================================================
// Imports
//==============================================================================

use std::{
    collections::{
        HashMap,
        VecDeque
    },
    net::SocketAddrV4, str::FromStr
};

use crate::{
    inetstack::protocols::{
        ipv4::Ipv4Header,
        tcp::segment::TcpHeader
    },
    runtime::memory::Buffer, MacAddress
};

use super::{connection::MigrationConnection, TcpMigrationHeader};

//==============================================================================
// Structures
//==============================================================================

pub struct TcpMigrationData {
    // Migrate-in data

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

    /// All possible targets to migrate out to.
    /// 
    /// target_addr -> connection
    pub targets: HashMap<SocketAddrV4, MigrationConnection>,
}

//==============================================================================
// Associated Functions
//==============================================================================

impl TcpMigrationData {
    pub fn new() -> Self {
        Self {
            recv_queues: HashMap::new(),
            ack_pending: HashMap::new(),
            locked: HashMap::new(),
            targets: HashMap::new(),
        }
    }

    fn send(&self, buf: Buffer) {

    }

    /// Check if this packet came from a target, and if so, perform migration actions.
    pub fn try_receive(&self, buf: Buffer) -> Result<(), &str> {
        //let migration_header = TcpMigrationHeader::deserialize(&buf)?;
        Ok(())
    }

    // TEMP

    pub fn get_target(&self) -> &MigrationConnection {
        self.targets.get(&SocketAddrV4::from_str("10.0.1.9:30000").unwrap()).unwrap()
    }

    pub fn should_migrate() -> bool {
        static mut FLAG: i32 = 0;
        unsafe {
            FLAG += 1;
            FLAG == 11
        }

    }
}