// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddrV4, time::Instant
};

//======================================================================================================================
// Constants
//======================================================================================================================

const WINDOW: usize = 5;

//======================================================================================================================
// Structures
//======================================================================================================================

type PacketsPerSec = f64;

struct Stat {
    instants: VecDeque<Instant>,
    value: PacketsPerSec,
}

pub struct TcpMigStats {
    global_incoming_traffic: Stat,

    /// Incoming traffic rate per connection.
    /// 
    /// (local, remote) -> requests per milli-second.
    incoming_traffic: HashMap<(SocketAddrV4, SocketAddrV4), Stat>,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl TcpMigStats {
    pub fn new() -> Self {
        Self {
            global_incoming_traffic: Stat::new(),
            incoming_traffic: HashMap::new(),
        }
    }

    pub fn update(&mut self, local: SocketAddrV4, remote: SocketAddrV4) {
        let instant = Instant::now();
        self.global_incoming_traffic.update(instant);
        self.incoming_traffic.entry((local, remote)).or_insert(Stat::new()).update(instant);
    }
}

impl Stat {
    fn new() -> Self {
        Self {
            instants: VecDeque::new(),
            value: 0.0,
        }
    }

    fn update(&mut self, instant: Instant) {
        self.instants.push_back(instant);
        if self.instants.len() > WINDOW {
            self.instants.pop_front().unwrap();
            self.value = (WINDOW as f64) / (*self.instants.back().unwrap() - *self.instants.front().unwrap()).as_secs_f64();
        }
        //eprintln!("{}", self.value);
    }
}