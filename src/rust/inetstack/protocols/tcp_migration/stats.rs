// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddrV4, time::Instant,
    fmt,
};

#[cfg(feature = "capybara-log")]
use crate::tcpmig_profiler::tcp_log;

//======================================================================================================================
// Constants
//======================================================================================================================

const WINDOW: usize = 50;

//======================================================================================================================
// Structures
//======================================================================================================================

struct PacketRate {
    instants: VecDeque<Instant>,
}

impl fmt::Debug for PacketRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut prev_time = None;
        let mut idx: i32 = 1;
        writeln!(f, "")?;
        for time in &self.instants {
            if let Some(prev) = prev_time {
                let time_diff = time.duration_since(prev);
                writeln!(f, "GAP#{}: {:?}", idx, time_diff)?;
                idx+=1;
            }
            prev_time = Some(*time);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
struct RollingAverageResult(u64);

struct RollingAverage {
    values: VecDeque<usize>,
    sum: usize,
}
impl fmt::Debug for RollingAverage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "\nvalues: {:?}\nsum: {}\naverage: {:?}\n", self.values, self.sum, self.get())
    }
}

pub struct TcpMigStats {
    global_incoming_traffic: PacketRate,
    global_outgoing_traffic: PacketRate,

    /// Incoming traffic rate per connection.
    /// 
    /// (local, client) -> requests per milli-second.
    recv_queue_lengths: HashMap<(SocketAddrV4, SocketAddrV4), RollingAverage>,
    global_recv_queue_length: u64,
    threshold: u64,
}

impl fmt::Debug for TcpMigStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TcpMigStats")
            .field("\nglobal_incoming_traffic", &self.global_incoming_traffic)
            .field("\nglobal_outgoing_traffic", &self.global_outgoing_traffic)
            .field("\nrecv_queue_lengths\n", &self.recv_queue_lengths)
            .finish()
    }
}

//======================================================================================================================
// Standard Library Trait Implementations
//======================================================================================================================

impl std::cmp::Eq for RollingAverageResult {}

impl std::cmp::Ord for RollingAverageResult {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).expect("RollingAverageResult should never be NaN")
    }
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl TcpMigStats {
    pub fn new(threshold: u64) -> Self {
        Self {
            global_incoming_traffic: PacketRate::new(),
            global_outgoing_traffic: PacketRate::new(),
            recv_queue_lengths: HashMap::new(),
            global_recv_queue_length: 0,
            threshold,
        }
    }

    pub fn num_of_connections(&self) -> usize {
        self.recv_queue_lengths.len()
    }

    pub fn update_incoming(&mut self, local: SocketAddrV4, client: SocketAddrV4, recv_queue_len: usize) {
        let instant = Instant::now();
        self.global_incoming_traffic.update(instant);
        
        match self.recv_queue_lengths.get_mut(&(local, client)) {
            Some(len_entry) => {
                let old_len = len_entry.get().0;
                len_entry.update(recv_queue_len);
                let new_len = len_entry.get().0;
                // println!("old: {}, new: {}", old_len, new_len);
                self.global_recv_queue_length = self.global_recv_queue_length + new_len - old_len;
                
            },
            None => {
                // Handle the case when the entry does not exist
                return; // or any other action to finish the function
            },
        }
    }

    pub fn update_outgoing(&mut self) {
        let instant = Instant::now();
        self.global_outgoing_traffic.update(instant);
    }

    pub fn global_recv_queue_length(&self) -> u64 {
        self.global_recv_queue_length
    }

    pub fn get_rx_tx_ratio(&self) -> f64 {
        self.global_incoming_traffic.get() / self.global_outgoing_traffic.get()
    }

    pub fn get_connection_to_migrate_out(&self) -> Option<(SocketAddrV4, SocketAddrV4)> {
        self.recv_queue_lengths.iter()
        .max_by_key(|(_, v)| v.get())
        .and_then(|(k, _)| Some(*k))

        // let pivot = self.global_recv_queue_length - self.threshold;

        // self.recv_queue_lengths.iter()
        //     .filter(|(_, v)| v.get().0 > pivot)
        //     .min_by_key(|(_, v)| v.get())
        //     .or_else(||
        //         self.recv_queue_lengths.iter()
        //             .max_by_key(|(_, v)| v.get())
        //     )
        //     .and_then(|(k, _)| Some(*k))
    }
    
    pub fn start_tracking_connection(&mut self, local: SocketAddrV4, client: SocketAddrV4) {
        #[cfg(feature = "capybara-log")]
        {
            tcp_log(format!("start"));
        }
        assert!(!self.recv_queue_lengths.contains_key(&(local, client)));
        self.recv_queue_lengths.insert((local, client), RollingAverage::new());
    }

    pub fn stop_tracking_connection(&mut self, local: SocketAddrV4, client: SocketAddrV4) {
        #[cfg(feature = "capybara-log")]
        {
            tcp_log(format!("stop"));
        }
        match self.recv_queue_lengths.remove(&(local, client)) {
            Some(len) => self.global_recv_queue_length -= len.get().0,
            None => warn!("`TcpMigStats` was not tracking connection ({}, {})", local, client),
        }
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

    fn get(&self) -> RollingAverageResult {
        if self.values.len() < WINDOW {
            RollingAverageResult(0)
        }
        else {
            RollingAverageResult((self.sum as u64) / (WINDOW as u64))
        }
    }
}