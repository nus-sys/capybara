// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::{segment::{
    TcpMigHeader,
    TcpMigSegment,
}, MigrationStage};
use crate::{
inetstack::protocols::{
        arp::ArpPeer,
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        ip::IpProtocol,
        ipv4::Ipv4Header,
    },
runtime::{
    fail::Fail,
    memory::Buffer,
    network::{
        types::MacAddress,
        NetworkRuntime,
    },
    QDesc,
},
scheduler::{
    Scheduler,
    SchedulerHandle,
},
};
use ::libc::{
EAGAIN,
EBADF,
EEXIST,
};
use ::std::{
collections::HashMap,
net::{
    Ipv4Addr,
    SocketAddrV4,
},
rc::Rc,
};

#[cfg(feature = "profiler")]
use crate::timer;

//======================================================================================================================
// Constants
//======================================================================================================================



//======================================================================================================================
// Structures
//======================================================================================================================

pub struct ActiveMigration {
    last_sent_stage: MigrationStage,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl ActiveMigration {
    pub fn new() -> Self {
        Self {
            last_sent_stage: MigrationStage::None,
        }
    }
}