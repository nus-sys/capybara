// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use std::{
    collections::{HashMap, VecDeque, hash_map::Entry, HashSet},
    net::SocketAddrV4, cell::Cell,
};

#[cfg(feature = "capybara-log")]
use crate::tcpmig_profiler::tcp_log;

//======================================================================================================================
// Constants
//======================================================================================================================

const GLOBAL_ROLLAVG_WINDOW_LOG2: usize = 10;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct RollingAverage {
    values: VecDeque<usize>,
    sum: usize,
    window_log2: usize,
}

struct BucketList {
    /// Index 0: Connections with queue length 1-9. (0-length connections are not stored)
    /// 
    /// Index 1: Connections with queue length 10-19.
    /// 
    /// Index 2: Connections with queue length 20-29, and so on.
    buckets: Vec<Vec<(SocketAddrV4, SocketAddrV4)>>,

    /// Mapping from connection to its position in `buckets`.
    positions: HashMap<(SocketAddrV4, SocketAddrV4), (usize, usize)>,
}

pub struct TcpMigStats {
    // Receive Queue Stats
    global_recv_queue_length: usize,
    avg_global_recv_queue_length: RollingAverage,
    threshold: usize,
    recv_queue_stats: BucketList,
    //recv_queue_lengths: HashMap<(SocketAddrV4, SocketAddrV4), RollingAverage>,
    connections: HashSet<(SocketAddrV4, SocketAddrV4)>,

    // Granularity
    pub granularity: i32,
}

//======================================================================================================================
// Standard Library Trait Implementations
//======================================================================================================================

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl TcpMigStats {
    pub fn new(threshold: usize) -> Self {
        let granularity = std::env::var("STATS_GRANULARITY").map_or(1, |val| val.parse().unwrap());
        Self {
            global_recv_queue_length: 0,
            avg_global_recv_queue_length: RollingAverage::new(GLOBAL_ROLLAVG_WINDOW_LOG2),
            threshold,
            recv_queue_stats: BucketList::new(),
            connections: HashSet::new(),
            granularity,
        }
    }

    pub fn num_of_connections(&self) -> usize {
        self.connections.len()
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

    /// Returns the updated granularity counter.
    pub(super) fn recv_queue_update(&mut self, connection: (SocketAddrV4, SocketAddrV4), is_increment: bool, new_queue_len: usize, granularity_counter: &Cell<i32>) {
        if is_increment {
            self.global_recv_queue_length += 1;
        } else {
            // Pop is always only called after at least one push.
            assert!(self.global_recv_queue_length > 0);
            self.global_recv_queue_length -= 1;
        }
        self.avg_global_recv_queue_length.update(self.global_recv_queue_length);

        // eprintln!("{}", self.global_recv_queue_length);
        let counter = granularity_counter.get() + 1;
        if counter >= self.granularity {
            granularity_counter.set(0);
            self.recv_queue_stats.update(connection, new_queue_len);
        } else {
            granularity_counter.set(counter);
        }
    }

    pub fn global_recv_queue_length(&self) -> usize {
        // eprintln!("{}", self.global_recv_queue_length);
        self.global_recv_queue_length
    }

    pub fn avg_global_recv_queue_length(&self) -> usize {
        // eprintln!("{}", self.avg_global_recv_queue_length.get());
        self.avg_global_recv_queue_length.get()
    }

    pub fn print_queue_length(&self) {
        /* for (idx, qlen) in &self.connections {
            println!("{:?},{}", idx, qlen.get());
        } */
        unimplemented!("print_queue_length()")
    }

    /// Needs global receive queue length to be greater than the threshold.
    pub fn get_connection_to_migrate_out(&mut self) -> Option<(SocketAddrV4, SocketAddrV4)> {
        /* self.recv_queue_lengths.iter()
        .max_by_key(|(_, v)| v.get())
        .and_then(|(k, _)| Some(*k)) */

        assert!(self.avg_global_recv_queue_length() >= self.threshold);

        if self.recv_queue_stats.positions.is_empty() {
            return None;
        }

        let pivot = self.avg_global_recv_queue_length() - self.threshold;
        eprintln!("pivot {} = {} - {}", pivot, self.avg_global_recv_queue_length(), self.threshold);
        Some(self.recv_queue_stats.pop_connection(pivot))

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
        assert!(self.connections.insert((local, client)));

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
            tcp_log(format!("stop tracking {:?}", (local, client)));
        }
        
        match self.connections.remove(&(local, client)) {
            false => warn!("`TcpMigStats` was not tracking connection ({}, {})", local, client),
            _ => (),
        }

        /* #[cfg(not(feature = "mig-per-n-req"))] {
            if self.global_recv_queue_length <= recv_queue_len {
                self.global_recv_queue_length = 0;
            }else{
                self.global_recv_queue_length -= recv_queue_len;
            }
            // assert!(self.global_recv_queue_counter >= 0);
            self.avg_global_recv_queue_length.update(self.global_recv_queue_length);
        } */

        self.recv_queue_stats.remove(&(local, client));
        self.force_decrease_global_queue_length(recv_queue_len);

        // Remove connection from bucket list.

        // Print all keys in recv_queue_lengths
        // println!("Keys in recv_queue_lengths:");
        // for key in self.recv_queue_lengths.keys() {
        //     println!("{:?}", key);
        // }
    }

    pub(super) fn force_decrease_global_queue_length(&mut self, by: usize) {
        if self.global_recv_queue_length <= by {
            self.global_recv_queue_length = 0;
        } else {
            self.global_recv_queue_length -= by;
        }
        // assert!(self.global_recv_queue_counter >= 0);
        //self.avg_global_recv_queue_length.force_set(self.global_recv_queue_length);
        self.avg_global_recv_queue_length.force_clear();
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
        let start_index = std::cmp::min(queue_length / Self::BUCKET_SIZE, self.buckets.len() - 1);
        // Iterate through the outer vector
        for (bucket_index, bucket) in self.buckets.iter().enumerate() {
            eprintln!("Bucket {}:", bucket_index);

            // Iterate through the inner vector
            for (index, &(addr1, addr2)) in bucket.iter().enumerate() {
                eprintln!("Element {}: ({}, {})", index, addr1, addr2);
            }
        }
        for bucket in self.buckets[start_index..].iter_mut() {
            if let Some(connection) = bucket.pop() {
                self.positions.remove(&connection).unwrap();
                eprintln!("conn: {:?}", connection);
                return connection;
            };
        }

        panic!("Invalid queue_length = {}", queue_length)
    }

    fn update(&mut self, connection: (SocketAddrV4, SocketAddrV4), new_queue_len: usize) {
        if new_queue_len == 0 {
            // Remove connection if connection existed previously.
            self.remove(&connection);
            return
        }
        
        let bucket_index = new_queue_len / Self::BUCKET_SIZE;
        let index = self.get_bucket_mut(bucket_index).len();

        match self.positions.entry(connection) {
            Entry::Vacant(entry) => {
                entry.insert((bucket_index, index));
            },

            Entry::Occupied(mut entry) => {
                let (old_bucket_index, old_index) = *entry.get();
                if old_bucket_index == bucket_index {
                    return
                }

                entry.insert((bucket_index, index));
                self.remove_from_bucket(old_bucket_index, old_index);
            },
        }
        self.add_to_bucket(bucket_index, connection);
    }

    fn remove(&mut self, connection: &(SocketAddrV4, SocketAddrV4)) {
        if let Some((bucket_index, index)) = self.positions.remove(connection) {
            self.remove_from_bucket(bucket_index, index);
        }
    }

    fn add_to_bucket(&mut self, bucket_index: usize, connection: (SocketAddrV4, SocketAddrV4)) -> usize {
        let bucket = self.get_bucket_mut(bucket_index);
        let index = bucket.len();
        bucket.push(connection);
        index
    }

    fn remove_from_bucket(&mut self, bucket_index: usize, index: usize) {
        let bucket = &mut self.buckets[bucket_index];
        bucket.swap_remove(index);
        if index < bucket.len() {
            self.positions.insert(bucket[index], (bucket_index, index));
        }
    }

    fn get_bucket_mut(&mut self, bucket_index: usize) -> &mut Vec<(SocketAddrV4, SocketAddrV4)> {
        if bucket_index >= self.buckets.len() {
            let additional = bucket_index + 1 - self.buckets.len();
            self.buckets.reserve(additional);
            for _ in 0..additional {
                self.buckets.push(Vec::new());
            }
        }
        &mut self.buckets[bucket_index]
    }

    /* pub fn increment(&mut self, connection: (SocketAddrV4, SocketAddrV4), new_queue_len: usize) {
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
    } */
}

impl RollingAverage {
    pub fn new(window_log2: usize) -> Self {
        Self {
            values: VecDeque::from(vec![0; 1 << window_log2]),
            sum: 0,
            window_log2,
        }
    }

    pub fn update(&mut self, value: usize) {
        self.sum -= unsafe { self.values.pop_front().unwrap_unchecked() };
        self.sum += value;
        self.values.push_back(value);
    }

    pub fn get(&self) -> usize {
        // if self.sum >> Self::WINDOW_LOG_2 > 50 {
        //     for value in self.values.iter() {
        //         eprint!("{} ", value);
        //     }
        //     eprintln!("\n\n");
        // }
        self.sum >> self.window_log2
    }

    fn force_set(&mut self, value: usize) {
        for e in &mut self.values {
            *e = value;
        }
        self.sum = value << self.window_log2;
    }

    fn force_clear(&mut self) {
        let (l, r) = self.values.as_mut_slices();
        unsafe {
            libc::memset(l.as_mut_ptr() as *mut libc::c_void, 0, l.len() * std::mem::size_of::<usize>());
            if !r.is_empty() {
                libc::memset(r.as_mut_ptr() as *mut libc::c_void, 0, r.len() * std::mem::size_of::<usize>());
            }
        }
        self.sum = 0;
    }
}