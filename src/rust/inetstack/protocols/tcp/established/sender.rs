// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::ControlBlock;
use crate::{
    inetstack::protocols::tcp::{
        segment::TcpHeader,
        SeqNumber,
    },
    runtime::{
        fail::Fail,
        memory::Buffer,
        watched::{
            WatchFuture,
            WatchedValue,
        },
    },
};
use ::libc::{
    EBUSY,
    EINVAL,
};
use ::std::{
    cell::{
        Cell,
        RefCell,
    },
    collections::VecDeque,
    convert::TryInto,
    fmt,
    time::{
        Duration,
        Instant,
    },
};

use crate::capy_log;

// Structure of entries on our unacknowledged queue.
// ToDo: We currently allocate these on the fly when we add a buffer to the queue.  Would be more efficient to have a
// buffer structure that held everything we need directly, thus avoiding this extra wrapper.
//
#[derive(Debug, Clone)]
pub struct UnackedSegment {
    pub bytes: Buffer,
    // Set to `None` on retransmission to implement Karn's algorithm.
    pub initial_tx: Option<Instant>,
}

/// Hard limit for unsent queue.
/// ToDo: Remove this.  We should limit the unsent queue by either having a (configurable) send buffer size (in bytes,
/// not segments) and rejecting send requests that exceed that, or by limiting the user's send buffer allocations.
const UNSENT_QUEUE_CUTOFF: usize = 1024;

// ToDo: Consider moving retransmit timer and congestion control fields out of this structure.
// ToDo: Make all public fields in this structure private.
pub struct Sender {
    //
    // Send Sequence Space:
    //
    //                     |<-----------------send window size----------------->|
    //                     |                                                    |
    //                send_unacked               send_next         send_unacked + send window
    //                     v                         v                          v
    // ... ----------------|-------------------------|--------------------------|--------------------------------
    //       acknowledged  |      unacknowledged     |     allowed to send      |  future sequence number space
    //
    // Note: In RFC 793 terminology, send_unacked is SND.UNA, send_next is SND.NXT, and "send window" is SND.WND.
    //

    // Sequence Number of the oldest byte of unacknowledged sent data.  In RFC 793 terms, this is SND.UNA.
    pub send_unacked: WatchedValue<SeqNumber>,

    // Queue of unacknowledged sent data.  RFC 793 calls this the "retransmission queue".
    unacked_queue: RefCell<VecDeque<UnackedSegment>>,

    // Sequence Number of the next data to be sent.  In RFC 793 terms, this is SND.NXT.
    send_next: WatchedValue<SeqNumber>,

    // This is the send buffer (user data we do not yet have window to send).
    unsent_queue: RefCell<VecDeque<Buffer>>,

    // ToDo: Remove this as soon as sender.rs is fixed to not use it to tell if there is unsent data.
    unsent_seq_no: WatchedValue<SeqNumber>,

    // Available window to send into, as advertised by our peer.  In RFC 793 terms, this is SND.WND.
    send_window: WatchedValue<u32>,
    send_window_last_update_seq: Cell<SeqNumber>, // SND.WL1
    send_window_last_update_ack: Cell<SeqNumber>, // SND.WL2

    // RFC 1323: Number of bits to shift advertised window, defaults to zero.
    window_scale: u8,

    // Maximum Segment Size currently in use for this connection.
    // ToDo: Revisit this once we support path MTU discovery.
    mss: usize,
}

impl fmt::Debug for Sender {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Sender")
            .field("send_unacked", &self.send_unacked)
            .field("send_next", &self.send_next)
            .field("unsent_seq_no", &self.unsent_seq_no)
            .field("send_window", &self.send_window)
            .field("window_scale", &self.window_scale)
            .field("mss", &self.mss)
            .finish()
    }
}

impl Sender {
    pub fn new(seq_no: SeqNumber, send_window: u32, window_scale: u8, mss: usize) -> Self {
        Self {
            send_unacked: WatchedValue::new(seq_no),
            unacked_queue: RefCell::new(VecDeque::new()),
            send_next: WatchedValue::new(seq_no),
            unsent_queue: RefCell::new(VecDeque::new()),
            unsent_seq_no: WatchedValue::new(seq_no),

            send_window: WatchedValue::new(send_window),
            send_window_last_update_seq: Cell::new(seq_no),
            send_window_last_update_ack: Cell::new(seq_no),

            window_scale,
            mss,
        }
    }

    pub fn get_mss(&self) -> usize {
        self.mss
    }

    pub fn get_send_window(&self) -> (u32, WatchFuture<u32>) {
        self.send_window.watch()
    }

    pub fn get_send_unacked(&self) -> (SeqNumber, WatchFuture<SeqNumber>) {
        self.send_unacked.watch()
    }

    pub fn get_send_next(&self) -> (SeqNumber, WatchFuture<SeqNumber>) {
        self.send_next.watch()
    }

    pub fn modify_send_next(&self, f: impl FnOnce(SeqNumber) -> SeqNumber) {
        self.send_next.modify(f)
    }

    pub fn get_unsent_seq_no(&self) -> (SeqNumber, WatchFuture<SeqNumber>) {
        self.unsent_seq_no.watch()
    }

    pub fn push_unacked_segment(&self, segment: UnackedSegment) {
        self.unacked_queue.borrow_mut().push_back(segment)
    }

    // This is the main TCP send routine.
    //
    pub fn send(&self, buf: Buffer, cb: &ControlBlock) -> Result<(), Fail> {
        
        // If the user is done sending (i.e. has called close on this connection), then they shouldn't be sending.
        //
        if cb.user_is_done_sending.get() {
            return Err(Fail::new(EINVAL, "Connection is closing"));
        }

        // Our API supports send buffers up to usize (variable, depends upon architecture) in size.  While we could
        // allow for larger send buffers, it is simpler and more practical to limit a single send to 1 GiB, which is
        // also the maximum value a TCP can advertise as its receive window (with maximum window scaling).
        // ToDo: the below check just limits a single send to 4 GiB, not 1 GiB.  Check this doesn't break anything.
        //
        // Review: Move this check up the stack (i.e. closer to the user)?
        //
        let mut buf_len: u32 = buf
            .len()
            .try_into()
            .map_err(|_| Fail::new(EINVAL, "buffer too large"))?;

        // ToDo: What we should do here:
        //
        // Conceptually, we should take the provided buffer and add it to the unsent queue.  Then calculate the amount
        // of data we're currently allowed to send (based off of the receiver's advertised window, our congestion
        // control algorithm, silly window syndrome avoidance algorithm, etc).  Finally, enter a loop where we compose
        // maximum sized segments from the data on the unsent queue and send them, saving a (conceptual) copy of the
        // sent data on the unacknowledged queue, until we run out of either window space or unsent data.
        //
        // Note that there are several shortcuts we can make to this conceptual approach to speed the common case.
        // Note also that this conceptual send code is almost identical to what our "background send" algorithm should
        // be doing, so we should just have a single function that we call from both places.
        //
        // The current code below just tries to send the provided buffer immediately (if allowed), otherwise it places
        // it on the unsent queue and that's it.
        //

        // Check for unsent data.
        if self.unsent_queue.borrow().is_empty() {
            // No unsent data queued up, so we can try to send this new buffer immediately.

            // Calculate amount of data in flight (SND.NXT - SND.UNA).
            let send_unacknowledged: SeqNumber = self.send_unacked.get();
            let send_next: SeqNumber = self.send_next.get();
            let sent_data: u32 = (send_next - send_unacknowledged).into();

            // ToDo: What limits buffer len to MSS?
            let in_flight_after_send: u32 = sent_data + buf_len;

            // Before we get cwnd for the check, we prompt it to shrink it if the connection has been idle.
            cb.congestion_control_on_cwnd_check_before_send();
            let cwnd: u32 = cb.congestion_control_get_cwnd();

            // The limited transmit algorithm can increase the effective size of cwnd by up to 2MSS.
            let effective_cwnd: u32 = cwnd + cb.congestion_control_get_limited_transmit_cwnd_increase();

            let win_sz: u32 = self.send_window.get();

            if win_sz > 0 && win_sz >= in_flight_after_send && effective_cwnd >= in_flight_after_send {
                if let Some(remote_link_addr) = cb.arp().try_query(cb.get_remote().ip().clone()) {
                    // This hook is primarily intended to record the last time we sent data, so we can later tell if
                    // the connection has been idle.
                    let rto: Duration = cb.rto();
                    cb.congestion_control_on_send(rto, sent_data);

                    // Prepare the segment and send it.
                    let mut header: TcpHeader = cb.tcp_header();
                    header.seq_num = send_next;
                    if buf_len == 0 {
                        // This buffer is the end-of-send marker.
                        // Set FIN and adjust sequence number consumption accordingly.
                        header.fin = true;
                        buf_len = 1;
                    } else {
                        header.psh = true;
                    }
                    trace!("Send immediate");
                    capy_log!("SEND immediate");
                    cb.emit(header, Some(buf.clone()), remote_link_addr);

                    // Update SND.NXT.
                    self.send_next.modify(|s| s + SeqNumber::from(buf_len));

                    // ToDo: We don't need to track this.
                    self.unsent_seq_no.modify(|s| s + SeqNumber::from(buf_len));

                    // Put the segment we just sent on the retransmission queue.
                    let unacked_segment = UnackedSegment {
                        bytes: buf,
                        initial_tx: Some(cb.clock.now()),
                    };
                    self.unacked_queue.borrow_mut().push_back(unacked_segment);

                    // Start the retransmission timer if it isn't already running.
                    if cb.get_retransmit_deadline().is_none() {
                        let rto: Duration = cb.rto();
                        cb.set_retransmit_deadline(Some(cb.clock.now() + rto));
                    }

                    return Ok(());
                } else {
                    warn!("no ARP cache entry for send");
                    capy_log!("ARP issue in send()");
                }
            }
        }

        // Too fast.
        // ToDo: We need to fix this the correct way: limit our send buffer size to the amount we're willing to buffer.
        capy_log!("WARNINIG: TOO FAST, unsent_queue_len: {}", self.unsent_queue.borrow().len());
        if self.unsent_queue.borrow().len() > UNSENT_QUEUE_CUTOFF {
            return Err(Fail::new(EBUSY, "too many packets to send"));
        }

        // Slow path: Delegating sending the data to background processing.
        trace!("Queueing Send for background processing");
        eprintln!("Queueing Send for background processing");
        self.unsent_queue.borrow_mut().push_back(buf);
        self.unsent_seq_no.modify(|s| s + SeqNumber::from(buf_len));

        Ok(())
    }

    /// Retransmits the earliest segment that has not (yet) been acknowledged by our peer.
    pub fn retransmit(&self, cb: &ControlBlock) {
        // Check that we have an unacknowledged segment.
        if let Some(segment) = self.unacked_queue.borrow_mut().front_mut() {
            // We're retransmitting this, so we can no longer use an ACK for it as an RTT measurement (as we can't tell
            // if the ACK is for the original or the retransmission).  Remove the transmission timestamp from the entry.
            segment.initial_tx.take();

            // Clone the segment data for retransmission.
            let data: Buffer = segment.bytes.clone();

            // ToDo: Issue #198 Repacketization - we should send a full MSS (and set the FIN flag if applicable).

            // Prepare and send the segment.
            if let Some(first_hop_link_addr) = cb.arp().try_query(cb.get_remote().ip().clone()) {
                let mut header: TcpHeader = cb.tcp_header();
                header.seq_num = self.send_unacked.get();
                if data.len() == 0 {
                    // This buffer is the end-of-send marker.  Retransmit the FIN.
                    header.fin = true;
                }else{
                    header.psh = true;
                }
                cb.emit(header, Some(data), first_hop_link_addr);
            }
        } else {
            // We shouldn't enter the retransmit routine with an empty unacknowledged queue.  So maybe we should assert
            // here?  But this is relatively benign if it happens, and could be the result of a race-condition or a
            // mismanaged retransmission timer, so asserting would be over-reacting.
            warn!("Retransmission with empty unacknowledged queue?");
        }
    }

    // Remove acknowledged data from the unacknowledged (a.k.a. retransmission) queue.
    //
    pub fn remove_acknowledged_data(&self, cb: &ControlBlock, bytes_acknowledged: u32, now: Instant) {
        let mut bytes_remaining: usize = bytes_acknowledged as usize;

        while bytes_remaining != 0 {
            if let Some(segment) = self.unacked_queue.borrow_mut().front_mut() {
                // Add sample for RTO if we have an initial transmit time.
                // Note that in the case of repacketization, an ack for the first byte is enough for the time sample.
                // ToDo: TCP timestamp support.
                if let Some(initial_tx) = segment.initial_tx {
                    cb.rto_add_sample(now - initial_tx);
                }

                if segment.bytes.len() > bytes_remaining {
                    // Only some of the data in this segment has been acked.  Remove just the acked amount.
                    segment.bytes.adjust(bytes_remaining);
                    segment.initial_tx = None;

                    // Leave this segment on the unacknowledged queue.
                    break;
                }

                if segment.bytes.len() == 0 {
                    // This buffer is the end-of-send marker.  So we should only have one byte of acknowledged sequence
                    // space remaining (corresponding to our FIN).
                    debug_assert_eq!(bytes_remaining, 1);
                    bytes_remaining = 0;
                }

                bytes_remaining -= segment.bytes.len();
            } else {
                debug_assert!(false); // Shouldn't have bytes_remaining with no segments remaining in unacked_queue.
            }

            // Remove this segment from the unacknowledged queue.
            // ToDo: Mark the send operation associated with this buffer as complete, so the user can reuse the buffer.
            self.unacked_queue.borrow_mut().pop_front();
        }
    }

    pub fn pop_one_unsent_byte(&self) -> Option<Buffer> {
        let mut queue = self.unsent_queue.borrow_mut();

        let buf = queue.front_mut()?;
        let mut cloned_buf = buf.clone();
        let buf_len: usize = buf.len();

        // Pop one byte off the buf still in the queue and all but one of the bytes on our clone.
        buf.adjust(1);
        cloned_buf.trim(buf_len - 1);

        Some(cloned_buf)
    }

    pub fn pop_unsent(&self, max_bytes: usize) -> Option<(Buffer, bool)> {
        // TODO: Use a scatter/gather array to coalesce multiple buffers into a single segment.
        let mut unsent_queue = self.unsent_queue.borrow_mut();
        let mut buf: Buffer = unsent_queue.pop_front()?;
        let mut do_push = true;
        let buf_len: usize = buf.len();

        if buf_len > max_bytes {
            let mut cloned_buf = buf.clone();

            buf.adjust(max_bytes);
            cloned_buf.trim(buf_len - max_bytes);

            unsent_queue.push_front(buf);
            buf = cloned_buf;

            // Suppress PSH flag for partial buffers
            do_push = false;
        }
        Some((buf, do_push))
    }

    pub fn top_size_unsent(&self) -> Option<usize> {
        let unsent_queue = self.unsent_queue.borrow_mut();
        Some(unsent_queue.front()?.len())
    }

    // Update our send window to the value advertised by our peer.
    //
    pub fn update_send_window(&self, header: &TcpHeader) {
        // Check that the ACK we're using to update the window isn't older than the last one used to update it.
        if self.send_window_last_update_seq.get() < header.seq_num
            || (self.send_window_last_update_seq.get() == header.seq_num
                && self.send_window_last_update_ack.get() <= header.ack_num)
        {
            // Update our send window.
            self.send_window.set((header.window_size as u32) << self.window_scale);
            self.send_window_last_update_seq.set(header.seq_num);
            self.send_window_last_update_ack.set(header.ack_num);
        }

        debug!(
            "Updating window size -> {} (hdr {}, scale {})",
            self.send_window.get(),
            header.window_size,
            self.window_scale
        );
    }

    pub fn remote_mss(&self) -> usize {
        self.mss
    }
}

//==============================================================================
// TCP Migration
//==============================================================================

#[cfg(feature = "tcp-migration")]
pub mod state {
    use std::{collections::VecDeque, cell::{RefCell, Cell}};
    use byteorder::{BigEndian, ByteOrder};

    use crate::{
        inetstack::protocols::tcp::{SeqNumber, peer::state::{Serialize, Deserialize}},
        runtime::{memory::Buffer, watched::WatchedValue}
    };
    use super::{UnackedSegment, Sender};

    //===================================================================
    //  Structures
    //===================================================================

    #[derive(Debug, PartialEq, Eq)]
    pub struct SenderState {
        unsent_seq_no: SeqNumber, // 0..4
        send_unacked: SeqNumber, // 4..8
        send_next: SeqNumber, // 8..12
        send_window: u32, // 12..16
        send_window_last_update_seq: SeqNumber, // 16..20
        send_window_last_update_ack: SeqNumber, // 20..24
        mss: u32, // 24..28
        window_scale: u8, // 28
        unacked_queue: VecDeque<UnackedSegment>, // 29..
        unsent_queue: VecDeque<Buffer>,
    }

    //===================================================================
    //  Standard Library Trait Implementations
    //===================================================================

    impl From<&Sender> for SenderState {
        fn from(Sender {
            send_unacked,
            unacked_queue,
            send_next,
            unsent_queue,
            unsent_seq_no,

            send_window,
            send_window_last_update_seq,
            send_window_last_update_ack,

            window_scale,
            mss,
        }: &Sender) -> Self {
            Self {
                unsent_seq_no: unsent_seq_no.get(),
                send_unacked: send_unacked.get(),
                send_next: send_next.get(),
                send_window: send_window.get(),
                send_window_last_update_seq: send_window_last_update_seq.get(),
                send_window_last_update_ack: send_window_last_update_ack.get(),
                mss: (*mss).try_into().expect("MSS too big"),
                window_scale: *window_scale,
                unacked_queue: unacked_queue.take(),
                unsent_queue: unsent_queue.take(),
            }
        }
    }

    impl From<SenderState> for Sender {
        fn from(SenderState {
            unsent_seq_no,
            send_unacked,
            send_next,
            send_window,
            send_window_last_update_seq,
            send_window_last_update_ack,
            mss,
            window_scale,
            unacked_queue,
            unsent_queue
        }: SenderState) -> Self {
            Self {
                send_unacked: WatchedValue::new(send_unacked),
                unacked_queue: RefCell::new(unacked_queue),
                send_next: WatchedValue::new(send_next),
                unsent_queue: RefCell::new(unsent_queue),
                unsent_seq_no: WatchedValue::new(unsent_seq_no),
                send_window: WatchedValue::new(send_window),
                send_window_last_update_seq: Cell::new(send_window_last_update_seq),
                send_window_last_update_ack: Cell::new(send_window_last_update_ack),
                window_scale,
                mss: mss.try_into().unwrap(),
            }
        }
    }

    impl PartialEq for UnackedSegment {
        fn eq(&self, other: &Self) -> bool {
            self.bytes == other.bytes
        }
    }

    impl Eq for UnackedSegment {}

    //===================================================================
    //  Trait Implementations
    //===================================================================

    //==============================
    //  Serialization
    //==============================

    impl Serialize for SenderState {
        fn serialize_into<'buf>(&self, buf: &'buf mut [u8]) -> &'buf mut [u8] {
            buf[0..4].copy_from_slice(&u32::from(self.unsent_seq_no).to_be_bytes());
            buf[4..8].copy_from_slice(&u32::from(self.send_unacked).to_be_bytes());
            buf[8..12].copy_from_slice(&u32::from(self.send_next).to_be_bytes());
            buf[12..16].copy_from_slice(&self.send_window.to_be_bytes());
            buf[16..20].copy_from_slice(&u32::from(self.send_window_last_update_seq).to_be_bytes());
            buf[20..24].copy_from_slice(&u32::from(self.send_window_last_update_ack).to_be_bytes());
            buf[24..28].copy_from_slice(&self.mss.to_be_bytes());
            buf[28] = self.window_scale;

            let buf = &mut buf[29..];
            let buf = self.unacked_queue.serialize_into(buf);
            self.unsent_queue.serialize_into(buf)
        }
    }

    impl Serialize for UnackedSegment {
        fn serialize_into<'buf>(&self, buf: &'buf mut [u8]) -> &'buf mut [u8] {
            self.bytes.serialize_into(buf)
        }
    }

    //==============================
    //  Deserialization
    //==============================

    impl Deserialize for SenderState {
        fn deserialize_from(buf: &mut Buffer) -> Self {
            let unsent_seq_no = SeqNumber::from(BigEndian::read_u32(&buf[0..4]));
            let send_unacked = SeqNumber::from(BigEndian::read_u32(&buf[4..8]));
            let send_next = SeqNumber::from(BigEndian::read_u32(&buf[8..12]));
            let send_window = BigEndian::read_u32(&buf[12..16]);
            let send_window_last_update_seq = SeqNumber::from(BigEndian::read_u32(&buf[16..20]));
            let send_window_last_update_ack = SeqNumber::from(BigEndian::read_u32(&buf[20..24]));
            let mss = BigEndian::read_u32(&buf[24..28]);
            let window_scale = buf[28];

            buf.adjust(29);
            let unacked_queue = VecDeque::<UnackedSegment>::deserialize_from(buf);
            let unsent_queue = VecDeque::<Buffer>::deserialize_from(buf);

            Self { unsent_seq_no, send_unacked, send_next, send_window, send_window_last_update_seq,
                send_window_last_update_ack, mss, window_scale, unacked_queue, unsent_queue }
        }
    }

    impl Deserialize for UnackedSegment {
        fn deserialize_from(buf: &mut Buffer) -> Self {
            let bytes = Buffer::deserialize_from(buf);
            Self { bytes, initial_tx: None }
        }
    }

    //===================================================================
    //  Implementations
    //===================================================================

    impl SenderState {
        pub fn serialized_size(&self) -> usize {
            29 // Fixed
            + 4 + self.unacked_queue.iter().fold(0, |acc, e| acc + 4 + e.bytes.len()) // unacked_queue
            + 4 + self.unsent_queue.iter().fold(0, |acc, e| acc + 4 + e.len()) // unsent_queue
        }
    }

    //===================================================================
    //  Unit Tests
    //===================================================================

    #[cfg(test)]
    pub mod test {
        use std::collections::VecDeque;

        use crate::{inetstack::protocols::tcp::{SeqNumber, established::UnackedSegment}, runtime::memory::{Buffer, DataBuffer}};

        use super::SenderState;

        fn get_buf() -> Buffer {
            Buffer::Heap(DataBuffer::from_slice(&vec![1; 64]))
        }

        pub fn get_state() -> SenderState {
            SenderState {
                unsent_seq_no: SeqNumber::from(10),
                send_unacked: SeqNumber::from(10),
                send_next: SeqNumber::from(10),
                send_window: 10,
                send_window_last_update_seq: SeqNumber::from(10),
                send_window_last_update_ack: SeqNumber::from(10),
                mss: 10,
                window_scale: 10,
                unacked_queue: VecDeque::from(vec![UnackedSegment { bytes: get_buf(), initial_tx: None }; 100]),
                unsent_queue: VecDeque::from(vec![get_buf(); 100]),
            }
        }
    }
}

#[cfg(feature = "tcp-migration")]
impl Sender {
    pub fn migrated_in(
        unsent_seq_no: SeqNumber,
        send_unacked: SeqNumber,
        send_next: SeqNumber,
        send_window: u32,
        send_window_last_update_seq: SeqNumber,
        send_window_last_update_ack: SeqNumber,
        window_scale: u8,
        mss: usize,
        unacked_queue: VecDeque<UnackedSegment>,
        unsent_queue: VecDeque<Buffer>,
    ) -> Self {
        Self {
            send_unacked: WatchedValue::new(send_unacked),
            unacked_queue: RefCell::new(unacked_queue),
            send_next: WatchedValue::new(send_next),
            unsent_queue: RefCell::new(unsent_queue),
            unsent_seq_no: WatchedValue::new(unsent_seq_no),

            send_window: WatchedValue::new(send_window),
            send_window_last_update_seq: Cell::new(send_window_last_update_seq),
            send_window_last_update_ack: Cell::new(send_window_last_update_ack),

            window_scale,
            mss,
        }
    }
    
    pub fn get_send_window_last_update_seq(&self) -> SeqNumber {
        self.send_window_last_update_seq.get()
    }

    pub fn get_send_window_last_update_ack(&self) -> SeqNumber {
        self.send_window_last_update_ack.get()
    }

    pub fn get_window_scale(&self) -> u8 {
        self.window_scale
    }

    pub fn take_unsent_queue(&self) -> VecDeque<Buffer> {
        let mut temp = VecDeque::<Buffer>::with_capacity(0);
        std::mem::swap(&mut temp, &mut *self.unsent_queue.borrow_mut());
        temp
    }

    pub fn take_unacked_queue(&self) -> VecDeque<UnackedSegment> {
        let mut temp = VecDeque::<UnackedSegment>::with_capacity(0);
        std::mem::swap(&mut temp, &mut *self.unacked_queue.borrow_mut());
        temp
    }
}