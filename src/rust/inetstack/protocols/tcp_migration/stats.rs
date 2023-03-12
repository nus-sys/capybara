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
struct RollingAverageResult(f64);

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
    /// (local, remote) -> requests per milli-second.
    recv_queue_lengths: HashMap<(SocketAddrV4, SocketAddrV4), RollingAverage>,
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

    pub fn get_connection_to_migrate_out(&self) -> Option<(SocketAddrV4, SocketAddrV4)> {
        self.recv_queue_lengths.iter()
        .max_by_key(|(_, v)| v.get())
        .and_then(|(k, _)| Some(*k))
    }

    pub fn stop_tracking_connection(&mut self, local: SocketAddrV4, remote: SocketAddrV4) {
        self.recv_queue_lengths.remove(&(local, remote)).expect("connection should have been tracked");
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
            RollingAverageResult(0.0)
        }
        else {
            RollingAverageResult((self.sum as f64) / (WINDOW as f64))
        }
    }
}