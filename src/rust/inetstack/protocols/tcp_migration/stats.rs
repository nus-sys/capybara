// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddrV4,
};

#[cfg(feature = "capybara-log")]
use crate::tcpmig_profiler::tcp_log;

//======================================================================================================================
// Constants
//======================================================================================================================

//======================================================================================================================
// Structures
//======================================================================================================================

struct RollingAverage {
    values: VecDeque<usize>,
    sum: usize,
}

struct BucketList {
    /// Index 0: Connections with queue length 1-9. (0-length connections are not stored)
    /// 
    /// Index 1: Connections with queue length 10-19.
    /// 
    /// Index 2: Connections with queue length 20-29, and so on.
    buckets: Vec<Vec<(SocketAddrV4, SocketAddrV4)>>,

    /// Mapping from connection to its index in its corresponding bucket.
    positions: HashMap<(SocketAddrV4, SocketAddrV4), usize>,
}

pub struct TcpMigStats {
    // Receive Queue Stats
    global_recv_queue_length: usize,
    avg_global_recv_queue_length: RollingAverage,
    threshold: usize,
    recv_queue_stats: BucketList,
    recv_queue_lengths: HashMap<(SocketAddrV4, SocketAddrV4), RollingAverage>,
}

//======================================================================================================================
// Standard Library Trait Implementations
//======================================================================================================================

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl TcpMigStats {
    pub fn new(threshold: usize) -> Self {
        Self {
            global_recv_queue_length: 0,
            avg_global_recv_queue_length: RollingAverage::new(),
            threshold,
            recv_queue_stats: BucketList::new(),
            recv_queue_lengths: HashMap::new(),
        }
    }

    pub fn num_of_connections(&self) -> usize {
        self.recv_queue_lengths.len()
    }

    /* pub fn update_incoming(&mut self, local: SocketAddrV4, client: SocketAddrV4, recv_queue_len: usize) {        
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
    } */

    pub fn recv_queue_push(&mut self, connection: (SocketAddrV4, SocketAddrV4), new_queue_len: usize) {
        self.global_recv_queue_length += 1;
        self.avg_global_recv_queue_length.update(self.global_recv_queue_length);
    }

    pub fn recv_queue_pop(&mut self, connection: (SocketAddrV4, SocketAddrV4), new_queue_len: usize) {
        // Pop is always only called after at least one push.
        // debug_assert!(self.global_recv_queue_length > 0);
        self.global_recv_queue_length -= 1;
        self.avg_global_recv_queue_length.update(self.global_recv_queue_length);
    }

    pub fn global_recv_queue_length(&self) -> usize {
        self.global_recv_queue_length
    }

    pub fn avg_global_recv_queue_length(&self) -> usize {
        self.avg_global_recv_queue_length.get()
    }

    pub fn print_queue_length(&self) {
        for (idx, qlen) in &self.recv_queue_lengths {
            println!("{:?},{}", idx, qlen.get());
        }
    }

    /// Needs global receive queue length to be greater than the threshold.
    pub fn get_connection_to_migrate_out(&mut self) -> (SocketAddrV4, SocketAddrV4) {
        /* self.recv_queue_lengths.iter()
        .max_by_key(|(_, v)| v.get())
        .and_then(|(k, _)| Some(*k)) */

        assert!(self.avg_global_recv_queue_length() >= self.threshold);
        let pivot = self.avg_global_recv_queue_length() - self.threshold;
        self.recv_queue_stats.pop_connection(pivot)

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
        assert!(self.recv_queue_lengths.insert((local, client), RollingAverage::new()).is_none());

        // Print all keys in recv_queue_lengths
        // println!("Keys in recv_queue_lengths:");
        // for key in self.recv_queue_lengths.keys() {
        //     println!("{:?}", key);
        // }
    }

    pub fn stop_tracking_connection(
        &mut self, 
        local: SocketAddrV4, 
        client: SocketAddrV4,
        #[cfg(not(feature = "mig-per-n-req"))] 
        recv_queue_len: usize,
    ) {
        #[cfg(feature = "capybara-log")]
        {
            tcp_log(format!("stop"));
        }
        
        match self.recv_queue_lengths.remove(&(local, client)) {
            None => warn!("`TcpMigStats` was not tracking connection ({}, {})", local, client),
            _ => (),
        }

        #[cfg(not(feature = "mig-per-n-req"))] {
            if self.global_recv_queue_length <= recv_queue_len {
                self.global_recv_queue_length = 0;
            }else{
                self.global_recv_queue_length -= recv_queue_len;
            }
            // assert!(self.global_recv_queue_counter >= 0);
            self.avg_global_recv_queue_length.update(self.global_recv_queue_length);
        }

        // Print all keys in recv_queue_lengths
        // println!("Keys in recv_queue_lengths:");
        // for key in self.recv_queue_lengths.keys() {
        //     println!("{:?}", key);
        // }
    }
}

impl BucketList {
    const BUCKET_SIZE: usize = 10;

    fn new() -> Self {
        Self {
            buckets: Vec::new(),
            positions: HashMap::new(),
        }
    }

    /// Removes and returns a connection from the bucket list for the corresponding queue length.
    fn pop_connection(&mut self, queue_length: usize) -> (SocketAddrV4, SocketAddrV4) {
        for bucket in self.buckets[queue_length / Self::BUCKET_SIZE..].iter_mut() {
            if let Some(connection) = bucket.pop() {
                self.positions.remove(&connection).unwrap();
                return connection;
            };
        }

        panic!("Invalid queue_length = {}", queue_length)
    }

    pub fn increment(&mut self, connection: (SocketAddrV4, SocketAddrV4), new_queue_len: usize) {
        if new_queue_len == 1 || new_queue_len % Self::BUCKET_SIZE == 0 {
            // Add connection to the new bucket.
            let new_bucket = new_queue_len / Self::BUCKET_SIZE;
            if self.buckets.len() < new_bucket + 1 { // if instead of while because of incremental updates.
                self.buckets.push(Vec::new());
            }

            let new_index = self.buckets[new_bucket].len();
            self.buckets[new_bucket].push(connection);
            let old_index = self.positions.insert(connection, new_index);

            // Remove connection from the previous bucket if it's not in the first bucket.
            if let Some(old_index) = old_index {
                let old_bucket = &mut self.buckets[new_bucket - 1];
                old_bucket.swap_remove(old_index);
                
                // Update the position of the swapped connection.
                self.positions.insert(old_bucket[old_index], old_index);
            }
        }
    }

    pub fn decrement(&mut self, connection: (SocketAddrV4, SocketAddrV4), new_queue_len: usize) {
        let old_index = if new_queue_len == 0 {
            self.positions.remove(&connection).unwrap()
        }
        else if (new_queue_len + 1) % Self::BUCKET_SIZE == 0 {
            // Add connection to the new bucket.
            let new_bucket = new_queue_len / Self::BUCKET_SIZE;
            let new_index = self.buckets[new_bucket].len();
            self.buckets[new_bucket].push(connection);
            self.positions.insert(connection, new_index).unwrap()
        }
        else {
            return;
        };

        // Remove connection from old bucket.
        let old_bucket = &mut self.buckets[(new_queue_len + 1) / Self::BUCKET_SIZE];
        old_bucket.swap_remove(old_index);
        self.positions.insert(old_bucket[old_index], old_index);
    }
}

impl RollingAverage {
    const WINDOW_LOG_2: usize = 7;
    const WINDOW: usize = 1 << Self::WINDOW_LOG_2;

    fn new() -> Self {
        Self {
            values: VecDeque::from([0; Self::WINDOW]),
            sum: 0,
        }
    }

    fn update(&mut self, value: usize) {
        self.sum -= unsafe { self.values.pop_front().unwrap_unchecked() };
        self.sum += value;
        self.values.push_back(value);
    }

    fn get(&self) -> usize {
        self.sum >> Self::WINDOW_LOG_2
    }
}