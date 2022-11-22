//==============================================================================
// Imports
//==============================================================================

use crate::{
    MacAddress,
    runtime::memory::Buffer,
};
use ::std::{
    net::SocketAddrV4,
};

//==============================================================================
// Structures
//==============================================================================

#[derive(Debug)]
enum WaitState {
    None,
    WaitingForPrepareAck,
    WaitingForState,
    WaitingForStateAck,
}

#[derive(Debug)]
pub struct MigrationConnection {
    local_link_addr: MacAddress,
    target_link_addr: MacAddress,
    local: SocketAddrV4,
    target: SocketAddrV4,

    wait_state: WaitState,
}

//==============================================================================
// Associated Functions
//==============================================================================

impl MigrationConnection {
    pub fn new(
        local_link_addr: MacAddress,
        target_link_addr: MacAddress,
        local: SocketAddrV4,
        target: SocketAddrV4,
    ) -> Self {
        Self {
            local_link_addr,
            target_link_addr,
            local,
            target,
            
            wait_state: WaitState::None,
        }
    }

    fn send(&self, buf: Buffer) {

    }

    pub fn receive(&self, buf: Buffer) {
        
    }
}