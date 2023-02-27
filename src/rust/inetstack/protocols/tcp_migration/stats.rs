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

struct PacketRate {
    instants: VecDeque<Instant>,
}

struct RollingAverage {
    values: VecDeque<usize>,
    sum: usize,
}

pub struct TcpMigStats {
    global_incoming_traffic: PacketRate,
    global_outgoing_traffic: PacketRate,

    /// Incoming traffic rate per connection.
    /// 
    /// (local, remote) -> requests per milli-second.
    recv_queue_lengths: HashMap<(SocketAddrV4, SocketAddrV4), RollingAverage>,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl TcpMigStats {
    pub fn new() -> Self {
        Self {
            global_incoming_traffic: PacketRate::new(),
            global_outgoing_traffic: PacketRate::new(),
            recv_queue_lengths: HashMap::new(),
        }
    }

    pub fn update_incoming(&mut self, local: SocketAddrV4, remote: SocketAddrV4, recv_queue_len: usize) {
        let instant = Instant::now();
        self.global_incoming_traffic.update(instant);
        self.recv_queue_lengths.entry((local, remote)).or_insert(RollingAverage::new()).update(recv_queue_len);
    }

    pub fn update_outgoing(&mut self) {
        let instant = Instant::now();
        self.global_outgoing_traffic.update(instant);
    }

    pub fn get_rx_tx_ratio(&self) -> f64 {
        self.global_incoming_traffic.get() / self.global_outgoing_traffic.get()
    }
}

impl PacketRate {
    fn new() -> Self {
        Self {
            instants: VecDeque::new(),
        }
    }

    fn update(&mut self, instant: Instant) {
        self.instants.push_back(instant);
        if self.instants.len() > WINDOW {
            self.instants.pop_front().unwrap();
        }
        //eprintln!("{}", self.value);
    }

    fn get(&self) -> f64 {
        if self.instants.len() < WINDOW {
            0.0
        }
        else {
            (WINDOW as f64) / (*self.instants.back().unwrap() - *self.instants.front().unwrap()).as_secs_f64()
        }
    }
}

impl RollingAverage {
    fn new() -> Self {
        Self {
            values: VecDeque::new(),
            sum: 0,
        }
    }

    fn update(&mut self, value: usize) {
        self.values.push_back(value);
        self.sum += value;

        if self.values.len() > WINDOW {
            self.sum -= self.values.pop_front().unwrap();
        }
        //eprintln!("{}", self.value);
    }

    fn get(&self) -> f64 {
        if self.values.len() < WINDOW {
            0.0
        }
        else {
            (self.sum as f64) / (WINDOW as f64)
        }
    }
}