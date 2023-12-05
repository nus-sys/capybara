
use std::{
    collections::VecDeque,
    net::SocketAddrV4,
    cell::Cell, rc::Rc, fmt::Debug,
};

use arrayvec::ArrayVec;

use crate::{capy_log_mig, capy_log};

//======================================================================================================================
// Constants
//======================================================================================================================

const BUCKET_SIZE_LOG2: usize = 3;

const ROLLING_AVG_WINDOW_LOG2: usize = 3;
const ROLLING_AVG_WINDOW: usize = 1 << ROLLING_AVG_WINDOW_LOG2;

pub const MAX_EXTRACTED_CONNECTIONS: usize = 20;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct Stats {
    /// Global recv queue length at any moment.
    global_recv_queue_length: usize,
    /// Rolling average of global recv queue length.
    avg_global_recv_queue_length: RollingAverage,
    /// Sorted bucket list of connections according to individual recv queue lengths.
    buckets: BucketList,
    /// List of stat handles to poll.
    handles: Vec<StatsHandle>,
    
    /// Global recv queue length threshold above which migration is triggered.
    #[cfg(not(feature = "manual-tcp-migration"))]
    threshold: usize,
    /// TEMP TEST: Max number of connections to migrate for this test.
    max_migrations: Option<i32>,
}

#[derive(Debug)]
struct BucketList {
    /// Index 0: Connections with queue length 1-9. (0-length connections are not stored)
    /// 
    /// Index 1: Connections with queue length 10-19.
    /// 
    /// Index 2: Connections with queue length 20-29, and so on.
    buckets: Vec<Vec<StatsHandle>>,
}

#[derive(Clone)]
pub struct StatsHandle {
    inner: Rc<StatsHandleInner>
}

#[derive(Debug, Clone, Copy, Default)]
enum Stat {
    #[default]
    Disabled,

    Enabled(usize),
    MarkedForDisable(usize),
}

struct StatsHandleInner {
    /// Connection endpoints.
    conn: (SocketAddrV4, SocketAddrV4),
    /// Tracked queue length. If None, stop tracking this stat.
    queue_len: Cell<Stat>,
    /// Signals to update stat queue_len with this queue_len.
    queue_len_to_update: Cell<Option<usize>>,
    /// Position of this stat in the bucket list. None if not present (queue_len is zero).
    bucket_list_position: Cell<Option<(usize, usize)>>,
    /// Index of this stat in the tracked handles list.
    handles_index: Cell<Option<usize>>,
}

pub struct RollingAverage {
    values: VecDeque<usize>,
    sum: usize,
}

//======================================================================================================================
// Standard Library Trait Implementations
//======================================================================================================================

impl Debug for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stats")
            .field("global_recv_queue_length", &self.global_recv_queue_length)
            .field("avg_global_recv_queue_length", &self.avg_global_recv_queue_length.get())
            .field("handles", &self.handles)
            .field("buckets", &self.buckets)
            .finish()
    }
}

impl Debug for StatsHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.as_ref();
        f.debug_struct("StatsHandle")
            .field("conn", &format_args!("({}, {})", inner.conn.0, inner.conn.1))
            .field("queue_len", &format_args!("{:?}", inner.queue_len.get()))
            .field("queue_len_to_update", &format_args!("{:?}", inner.queue_len_to_update.get()))
            .field("bucket_list_position", &format_args!("{:?}", inner.bucket_list_position.get()))
            .field("handles_index", &format_args!("{:?}", inner.handles_index.get()))
            .finish()
    }
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl Stats {
    pub fn new() -> Self {
        Self {
            global_recv_queue_length: 0,
            avg_global_recv_queue_length: RollingAverage::new(),
            buckets: BucketList::new(),
            handles: Vec::new(),
            
            #[cfg(not(feature = "manual-tcp-migration"))]
            threshold: std::env::var("RECV_QUEUE_THRESHOLD").expect("No RECV_QUEUE_THRESHOLD specified")
                .parse().expect("RECV_QUEUE_THRESHOLD should be a number"),

            max_migrations: std::env::var("MAX_STAT_MIGS")
                .map(|e| Some(e.parse().expect("MAX_STAT_MIGS should be a number")))
                .unwrap_or(None),
        }
    }

    pub fn global_recv_queue_length(&self) -> usize {
        self.global_recv_queue_length
    }

    /// Extracts connections so that the global recv queue length goes below the threshold.
    /// Returns `None` if no connections need to be migrated.
    #[cfg(not(feature = "manual-tcp-migration"))]
    pub fn connections_to_migrate(&mut self) -> Option<ArrayVec<(SocketAddrV4, SocketAddrV4), MAX_EXTRACTED_CONNECTIONS>> {
        if let Some(val) = self.max_migrations {
            if val <= 0 {
                return None;
            }
        }

        if self.avg_global_recv_queue_length.get() <= self.threshold || self.global_recv_queue_length <= self.threshold {
            return None;
        }

        // Reset global avg.
        self.avg_global_recv_queue_length.force_set(0);

        let mut conns = ArrayVec::new();

        let mut should_end = self.global_recv_queue_length <= self.threshold || conns.remaining_capacity() == 0;
        let base_index = std::cmp::min((self.global_recv_queue_length - self.threshold) >> BUCKET_SIZE_LOG2, self.buckets.buckets.len());

        capy_log_mig!("Need to migrate: base_index({})\nbuckets: {:#?}", base_index, self.buckets.buckets);

        let (left, right) = self.buckets.buckets.split_at_mut(base_index);
        let iter = right.iter_mut().chain(left.iter_mut().rev());

        'outer: for bucket in iter {
            while let Some(handle) = bucket.pop() {
                capy_log_mig!("Chose: {:#?}", handle);

                // Remove handle.
                let handle_index = handle.inner.handles_index.get().unwrap();
                self.handles.swap_remove(handle_index);
                if handle_index < self.handles.len() {
                    self.handles[handle_index].inner.handles_index.set(Some(handle_index));
                }

                self.global_recv_queue_length -= handle.inner.queue_len.get().expect_enabled("bucket list stat not enabled");
                conns.push(handle.inner.conn);
                should_end = self.global_recv_queue_length <= self.threshold || conns.remaining_capacity() == 0;

                
                if let Some(val) = self.max_migrations.as_mut() {
                    *val -= 1;
                    if *val <= 0 {
                        break 'outer;
                    }
                }
                
                if should_end {
                    break 'outer;
                }
            }
        }

        self.avg_global_recv_queue_length.update(self.global_recv_queue_length);

        Some(conns)
    }

    pub fn poll(&mut self) {
        let mut i = 0;
        let mut f = false;
        while i < self.handles.len() {
            let inner = self.handles[i].inner.as_ref();

            // This handle is disabled, remove it.
            let queue_len = match inner.queue_len.get() {
                Stat::MarkedForDisable(len) => {
                    if !f {
                        f = true;
                        //capy_log_mig!("{:#?}", self);
                    }
    
                    capy_log!("Disable stats for {:?}", inner.conn);
    
                    let position = inner.bucket_list_position.get();
    
                    // Remove from handles. It will not be polled anymore.
                    self.handles.swap_remove(i);
                    if i < self.handles.len() {
                        self.handles[i].inner.handles_index.set(Some(i));
                    }
    
                    // Remove from buckets.
                    if let Some(position) = position {
                        self.buckets.remove(position);
                    }

                    // Update global queue length.
                    self.global_recv_queue_length -= len;

                    continue;
                },

                Stat::Enabled(len) => len,
                Stat::Disabled => panic!("disabled stat found in handles vector"),
            };

            // This handle was updated.
            if let Some(queue_len_to_update) = inner.queue_len_to_update.take() {
                if !f {
                    f = true;
                    //capy_log_mig!("{:#?}", self.buckets);
                }
                // Only update if the new value is different from the old value.
                if queue_len_to_update != queue_len {
                    capy_log!("Update stats for {:?}", inner.conn);
                    
                    // Update global queue length.
                    self.global_recv_queue_length = (self.global_recv_queue_length + queue_len_to_update) - queue_len;
                    
                    inner.queue_len.set(Stat::Enabled(queue_len_to_update));

                    // Update buckets.
                    self.buckets.update(&self.handles[i], queue_len_to_update);
                }
            }

            i += 1;
        }

        // Update rolling average of global queue length.
        self.avg_global_recv_queue_length.update(self.global_recv_queue_length);

        if f {
            capy_log_mig!("global queue len = {}", self.global_recv_queue_length);
            capy_log_mig!("avg global queue len = {}", self.avg_global_recv_queue_length.get());
            //capy_log_mig!("{:#?}", &self.buckets);
        }
    }
}

impl BucketList {
    fn new() -> Self {
        Self { buckets: Vec::new() }
    }

    fn insert(&mut self, handle: &StatsHandle, val: usize) {
        if val > 0 {
            let index = val >> BUCKET_SIZE_LOG2;
            let bucket = self.get_bucket_mut(index);
            let sub_index = bucket.len();

            // Set its position in buckets in the handle.
            handle.inner.bucket_list_position.set(Some((index, sub_index)));

            bucket.push(handle.clone());
        }
    }

    fn remove(&mut self, position: (usize, usize)) -> StatsHandle {
        let (index, sub_index) = position;
        let bucket = &mut self.buckets[index];
        // Remove handle from position.
        let removed = bucket.swap_remove(sub_index);
        removed.inner.bucket_list_position.take();
        // Update the handle newly moved to that position.
        if sub_index < bucket.len() {
            bucket[sub_index].inner.bucket_list_position.set(Some((index, sub_index)));
        }
        removed
    }

    /// Returns the new position.
    fn update(&mut self, handle: &StatsHandle, val: usize) {
        let position = handle.inner.bucket_list_position.get();
        let new_index = val >> BUCKET_SIZE_LOG2;

        match position {
            // Does not exist in buckets.
            None => {
                // Must be added to buckets.
                if val > 0 {
                    self.push_into_bucket(new_index, handle.clone());
                }
            },

            // Exists in buckets.
            Some((index, sub_index)) => {
                // Needs to be moved.
                if val == 0 || index != new_index {
                    // Remove from old bucket.
                    let handle = self.remove((index, sub_index));

                    // Must be moved to the new bucket.
                    if val > 0 {
                        self.push_into_bucket(new_index, handle);
                    }
                }
            }
        }
        //capy_log_mig!("Stats Update Iteration: {:#?}", self.buckets);
    }

    #[inline]
    fn push_into_bucket(&mut self, index: usize, handle: StatsHandle) {
        let bucket = self.get_bucket_mut(index);
        let sub_index = bucket.len();
        let position = (index, sub_index);
        handle.inner.bucket_list_position.set(Some(position));
        bucket.push(handle);
    }

    #[inline]
    fn get_bucket_mut(&mut self, index: usize) -> &mut Vec<StatsHandle> {
        if index >= self.buckets.len() {
            self.buckets.resize(index + 1, Vec::new());
        }
        &mut self.buckets[index]
    }
}

impl StatsHandle {
    pub fn new(conn: (SocketAddrV4, SocketAddrV4)) -> Self {
        Self {
            inner: Rc::new(StatsHandleInner {
                conn,
                queue_len: Cell::new(Stat::Disabled),
                queue_len_to_update: Cell::new(None),
                bucket_list_position: Cell::new(None),
                handles_index: Cell::new(None),
            })
        }
    }

    #[inline]
    pub fn set(&self, val: usize) {
        let inner= &self.inner;
        if let Stat::Enabled(..) = inner.queue_len.get() {
            inner.queue_len_to_update.set(Some(val));
        }
        // capy_log_mig!("Setting stat val={val}: {:#?}", self);
    }

    /// Adds handle to list of polled stats handles.
    pub fn enable(&self, stats: &mut Stats, initial_val: usize) {
        capy_log!("Enable stats for {:?}", self.inner.conn);
        self.inner.queue_len.set(Stat::Enabled(initial_val));

        // Add to handles.
        self.inner.handles_index.set(Some(stats.handles.len()));
        stats.handles.push(self.clone());

        // Add to buckets.
        stats.buckets.insert(self, initial_val);

        // Update global queue length.
        stats.global_recv_queue_length += initial_val;
    }

    /// Marks handle for removal from stats.
    pub fn disable(&self) {
        if let Stat::Enabled(val) = self.inner.queue_len.get() {
            capy_log!("Mark stats disabled for {:?}", self.inner.conn);
            self.inner.queue_len.set(Stat::MarkedForDisable(val));
            self.inner.handles_index.set(None);
        }
    }
}

impl Stat {
    fn expect_enabled(self, msg: &str) -> usize {
        if let Stat::Enabled(val) = self {
            return val;
        }
        panic!("Stat::expect_enabled() failed with `{}`", msg);
    }
}

impl RollingAverage {
    fn new() -> Self {
        Self {
            values: {
                let mut queue = VecDeque::with_capacity(ROLLING_AVG_WINDOW);
                queue.resize(ROLLING_AVG_WINDOW, 0);
                queue
            },
            sum: 0,
        }
    }

    #[inline]
    fn update(&mut self, value: usize) {
        self.sum -= unsafe { self.values.pop_front().unwrap_unchecked() };
        self.sum += value;
        self.values.push_back(value);
    }

    #[inline]
    fn get(&self) -> usize {
        self.sum >> ROLLING_AVG_WINDOW_LOG2
    }

    fn force_set(&mut self, value: usize) {
        for e in &mut self.values {
            *e = value;
        }
        self.sum = value << ROLLING_AVG_WINDOW_LOG2;
    }
}