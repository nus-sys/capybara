// Copyright (c) 2023 The TQUIC Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! BBR Congestion Control.
//!
//! BBR uses recent measurements of a transport connection's delivery rate
//! and round-trip time to build an explicit model that includes both the
//! maximum recent bandwidth available to that connection, and its
//! minimum recent round-trip delay.  BBR then uses this model to control
//! both how fast it sends data and the maximum amount of data it allows
//! in flight in the network at any time.
//!
//! See <https://datatracker.ietf.org/doc/html/draft-cardwell-iccrg-bbr-congestion-control-00>.
#![allow(unused_variables)]

use std::{
    cell::{
        Cell,
        RefCell,
    }, collections::VecDeque, rc::Rc, time::{
        Duration,
        Instant,
    }
};

//use log::*;
use rand::Rng;

use super::{
    CongestionControl,
    CongestionControlConstructor,
    FastRetransmitRecovery,
    LimitedTransmit,
    SlowStartCongestionAvoidance,
};

/// BBR configurable parameters.
#[derive(Debug)]
struct BbrConfig {
    /// Minimal congestion window in bytes.
    min_cwnd: u32,

    /// Initial congestion window in bytes.
    initial_cwnd: u32,

    /// Initial Smoothed rtt.
    initial_rtt: Option<Duration>,

    /// The minimum duration for ProbeRTT state
    probe_rtt_duration: Duration,

    /// If true, use a cwnd of `probe_rtt_cwnd_gain*BDP` during ProbeRtt state
    /// instead of minimal window.
    probe_rtt_based_on_bdp: bool,

    /// The cwnd gain for ProbeRTT state
    probe_rtt_cwnd_gain: f64,

    /// The length of the RTProp min filter window
    rtprop_filter_len: Duration,

    /// The cwnd gain for ProbeBW state
    probe_bw_cwnd_gain: f64,

    /// Max datagram size in bytes.
    max_datagram_size: u32,
}

/// Default outgoing udp datagram payloads size.
const DEFAULT_SEND_UDP_PAYLOAD_SIZE: usize = 1200;

/// Resumed connections over the same network MAY use the previous connection's
/// final smoothed RTT value as the resumed connection's initial RTT. When no
/// previous RTT is available, the initial RTT SHOULD be set to 333 milliseconds.
/// This results in handshakes starting with a PTO of 1 second, as recommended
/// for TCP's initial RTO
const INITIAL_RTT: Duration = Duration::from_millis(333);

impl Default for BbrConfig {
    fn default() -> Self {
        Self {
            min_cwnd: 4 * DEFAULT_SEND_UDP_PAYLOAD_SIZE as u32,
            initial_cwnd: 80 * DEFAULT_SEND_UDP_PAYLOAD_SIZE as u32,
            initial_rtt: Some(INITIAL_RTT),
            probe_rtt_duration: PROBE_RTT_DURATION,
            probe_rtt_based_on_bdp: false,
            probe_rtt_cwnd_gain: 0.75,
            rtprop_filter_len: RTPROP_FILTER_LEN,
            probe_bw_cwnd_gain: 2.0,
            max_datagram_size: DEFAULT_SEND_UDP_PAYLOAD_SIZE as u32,
        }
    }
}

/// BtlBwFilterLen: A constant specifying the length of the BBR.BtlBw max
/// filter window for BBR.BtlBwFilter, BtlBwFilterLen is `10` packet-timed
/// round trips.
const BTLBW_FILTER_LEN: u32 = 10;

/// RTpropFilterLen: A constant specifying the length of the RTProp min
/// filter window, RTpropFilterLen is `10` secs.
const RTPROP_FILTER_LEN: Duration = Duration::from_secs(10);

/// BBRHighGain: A constant specifying the minimum gain value that will
/// allow the sending rate to double each round (`2/ln(2)` ~= `2.89`), used
/// in Startup mode for both BBR.pacing_gain and BBR.cwnd_gain.
const HIGH_GAIN: f64 = 2.89;

/// Bandwidth growth rate before pipe got filled.
const BTLBW_GROWTH_RATE: f64 = 0.25;

/// Max count of full bandwidth reached, before pipe is supposed to be filled.
/// This three-round threshold was validated by YouTube experimental data.
const FULL_BW_COUNT_THRESHOLD: u32 = 3;

/// BBRGainCycleLen: the number of phases in the BBR ProbeBW gain cycle:
/// 8.
const GAIN_CYCLE_LEN: usize = 8;

/// Pacing Gain Cycles. Each phase normally lasts for roughly BBR.RTprop.
const PACING_GAIN_CYCLE: [f64; GAIN_CYCLE_LEN] = [1.25, 0.75, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];

/// ProbeRTTInterval: A constant specifying the minimum time interval
/// between ProbeRTT states: 10 secs.
const PROBE_RTT_INTERVAL: Duration = Duration::from_secs(10);

/// ProbeRTTDuration: A constant specifying the minimum duration for
/// which ProbeRTT state holds inflight to BBRMinPipeCwnd or fewer
/// packets: 200 ms.
const PROBE_RTT_DURATION: Duration = Duration::from_millis(200);

/// Pacing rate threshold for select different send quantum. Default `1.2Mbps`.
const SEND_QUANTUM_THRESHOLD_PACING_RATE: u32 = 1_200_000 / 8;

/// BBR State Machine.
///
/// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 3.4.
#[derive(Debug, PartialEq, Eq)]
enum BbrStateMachine {
    Startup,
    Drain,
    ProbeBW,
    ProbeRTT,
}

/// Round trip counter for tracking packet-timed round trips which starts
/// at the transmission of some segment, and then end at the ack of that segment.
#[derive(Debug, Default)]
struct RoundTripCounter {
    /// BBR.round_count: Count of packet-timed round trips.
    pub round_count: u32,

    /// BBR.round_start: A boolean that BBR sets to true once per packet-
    /// timed round trip, on ACKs that advance BBR.round_count.
    pub is_round_start: bool,

    /// BBR.next_round_delivered: packet.delivered value denoting the end of
    /// a packet-timed round trip.
    pub next_round_delivered: u32,
}

/// Full pipe estimator, used mainly during Startup mode.
#[derive(Debug, Default)]
struct FullPipeEstimator {
    /// BBR.filled_pipe: A boolean that records whether BBR estimates that it
    /// has ever fully utilized its available bandwidth ("filled the pipe").
    is_filled_pipe: bool,

    /// Baseline level delivery rate for full pipe estimator.
    full_bw: u32,

    /// The number of round for full pipe estimator without much growth.
    full_bw_count: u32,
}

/// Accumulate information from a single ACK/SACK.
#[derive(Debug)]
struct AckState {
    /// Ack time.
    now: Instant,

    /// Newly marked lost data size in bytes.
    newly_lost_bytes: u32,

    /// Newly acked data size in bytes.
    newly_acked_bytes: u32,

    /// The last P.delivered in bytes.
    packet_delivered: u32,

    /// The last P.sent_time to determine whether exit recovery.
    last_ack_packet_sent_time: Instant,

    /// The amount of data that was in flight before processing this ACK.
    prior_bytes_in_flight: u32,
}

impl Default for AckState {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            now,
            newly_lost_bytes: 0,
            newly_acked_bytes: 0,
            packet_delivered: 0,
            last_ack_packet_sent_time: now,
            prior_bytes_in_flight: 0,
        }
    }
}

/// BBR Congestion Control Algorithm.
///
/// See draft-cardwell-iccrg-bbr-congestion-control-00.
#[derive(Debug)]
struct BbrInner {
    /// Configurable parameters.
    config: BbrConfig,

    /// Statistics.
    stats: CongestionStats,

    /// State.
    state: BbrStateMachine,

    /// BBR.pacing_rate: The current pacing rate for a BBR flow, which
    /// controls inter-packet spacing.
    pacing_rate: u32,

    /// BBR.send_quantum: The maximum size of a data aggregate scheduled and
    /// transmitted together.
    send_quantum: u32,

    /// Cwnd: The transport sender's congestion window, which limits the
    /// amount of data in flight.
    //cwnd: u32,
    pub cwnd: std::rc::Rc<WatchedValue<u32>>,

    /// BBR.BtlBw: BBR's estimated bottleneck bandwidth available to the transport
    /// flow, estimated from the maximum delivery rate sample in a sliding window.
    btlbw: u32,

    /// BBR.BtlBwFilter: The max filter used to estimate BBR.BtlBw.
    btlbwfilter: MinMax,

    /// Delivery rate estimator.
    delivery_rate_estimator: DeliveryRateEstimator,

    /// BBR.RTprop: BBR's estimated two-way round-trip propagation delay of path,
    /// estimated from the windowed minimum recent round-trip delay sample.
    rtprop: Duration,

    /// BBR.rtprop_stamp: The wall clock time at which the current BBR.RTProp
    /// sample was obtained.
    rtprop_stamp: Instant,

    /// BBR.rtprop_expired: A boolean recording whether the BBR.RTprop has
    /// expired and is due for a refresh with an application idle period or a
    /// transition into ProbeRTT state.
    is_rtprop_expired: bool,

    /// BBR.pacing_gain: The dynamic gain factor used to scale BBR.BtlBw to
    /// produce BBR.pacing_rate.
    pacing_gain: f64,

    /// BBR.cwnd_gain: The dynamic gain factor used to scale the estimated
    /// BDP to produce a congestion window (cwnd).
    cwnd_gain: f64,

    /// Counter of packet-timed round trips.
    round: RoundTripCounter,

    /// Estimator of full pipe.
    full_pipe: FullPipeEstimator,

    /// Timestamp when ProbeRTT state ends.
    probe_rtt_done_stamp: Option<Instant>,

    /// Whether a roundtrip in ProbeRTT state ends.
    probe_rtt_round_done: bool,

    /// Whether in packet conservation mode.
    packet_conservation: bool,

    /// Cwnd before loss recovery.
    prior_cwnd: u32,

    /// Whether restarting from idle.
    is_idle_restart: bool,

    /// Last time when cycle_index is updated.
    cycle_stamp: Instant,

    /// Current index of pacing_gain_cycle[].
    cycle_index: usize,

    /// The upper bound on the volume of data BBR allows in flight.
    target_cwnd: u32,

    /// Whether in the recovery mode.
    in_recovery: bool,

    /// Accumulate information from a single ACK/SACK.
    ack_state: AckState,

    /// Time of the last recovery event starts.
    recovery_epoch_start: Option<Instant>,
}

impl BbrInner {
    pub fn new(config: BbrConfig) -> (Self, Rc<WatchedValue<u32>>) {
        let now = Instant::now();
        let initial_cwnd = config.initial_cwnd;

        let cwnd = Rc::new(WatchedValue::new(initial_cwnd));

        let mut bbr = Self {
            config,
            stats: Default::default(),
            state: BbrStateMachine::Startup,
            pacing_rate: 0,
            send_quantum: 0,
            cwnd: cwnd.clone(),
            btlbw: 0,
            btlbwfilter: MinMax::new(BTLBW_FILTER_LEN),
            delivery_rate_estimator: DeliveryRateEstimator::default(),
            rtprop: Duration::MAX,
            rtprop_stamp: now,
            is_rtprop_expired: false,
            pacing_gain: HIGH_GAIN,
            cwnd_gain: HIGH_GAIN,
            round: Default::default(),
            full_pipe: Default::default(),
            probe_rtt_done_stamp: None,
            probe_rtt_round_done: false,
            packet_conservation: false,
            prior_cwnd: 0,
            is_idle_restart: false,
            cycle_stamp: now,
            cycle_index: 0,
            target_cwnd: 0,
            in_recovery: false,
            ack_state: AckState::default(),
            recovery_epoch_start: None,
        };
        bbr.init();

        (bbr, cwnd)
    }

    /// Initialization Steps.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.3.1.
    fn init(&mut self) {
        self.rtprop = self.config.initial_rtt.unwrap_or(Duration::MAX);
        self.rtprop_stamp = Instant::now();
        self.probe_rtt_done_stamp = None;
        self.probe_rtt_round_done = false;
        self.packet_conservation = false;

        self.prior_cwnd = 0;
        self.is_idle_restart = false;

        self.init_round_counting();
        self.init_full_pipe();
        self.init_pacing_rate();
        self.enter_startup();
    }

    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.1.1.3.
    fn init_round_counting(&mut self) {
        self.round.next_round_delivered = 0;
        self.round.round_count = 0;
        self.round.is_round_start = false;
    }

    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.3.2.2.
    fn init_full_pipe(&mut self) {
        self.full_pipe.is_filled_pipe = false;
        self.full_pipe.full_bw = 0;
        self.full_pipe.full_bw_count = 0;
    }

    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.2.1
    fn init_pacing_rate(&mut self) {
        // When a BBR flow starts it has no BBR.BtlBw estimate.  So in this case
        // it sets an initial pacing rate based on the transport sender implementation's
        // initial congestion window, the initial SRTT (smoothed round-trip time) after the
        // first non-zero RTT sample.
        let srtt = match self.config.initial_rtt {
            Some(rtt) => rtt,
            _ => Duration::from_millis(1),
        };
        let nominal_bandwidth = self.config.initial_cwnd as f64 / srtt.as_secs_f64();
        self.pacing_rate = (self.pacing_gain * nominal_bandwidth) as u32;
    }

    /// Enter the Startup state
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.3.2.1.
    fn enter_startup(&mut self) {
        self.state = BbrStateMachine::Startup;

        // To achieve this rapid probing in the smoothest possible fashion, upon
        // entry into Startup state BBR sets BBR.pacing_gain and BBR.cwnd_gain
        // to BBRHighGain, the minimum gain value that will allow the sending
        // rate to double each round.
        self.pacing_gain = HIGH_GAIN;
        self.cwnd_gain = HIGH_GAIN;
    }

    /// Estimate whether the pipe is full by looking for a plateau in the
    /// BBR.BtlBw estimate.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.3.2.2.
    fn check_full_pipe(&mut self) {
        // no need to check for a full pipe now
        if self.is_filled_pipe() || !self.is_round_start() || self.delivery_rate_estimator.is_sample_app_limited() {
            return;
        }

        // BBR.BtlBw still growing?
        if self.btlbw >= (self.full_pipe.full_bw as f64 * (1.0_f64 + BTLBW_GROWTH_RATE)) as u32 {
            // record new baseline level
            self.full_pipe.full_bw = self.btlbw;
            self.full_pipe.full_bw_count = 0;
            return;
        }

        // another round w/o much growth
        self.full_pipe.full_bw_count += 1;

        // BBR waits three rounds in order to have solid evidence that the
        // sender is not detecting a delivery-rate plateau that was temporarily
        // imposed by the receive window.
        // This three-round threshold was validated by YouTube experimental data.
        if self.full_pipe.full_bw_count >= FULL_BW_COUNT_THRESHOLD {
            self.full_pipe.is_filled_pipe = true;
        }
    }

    /// Update the virtual time tracked by BBR.round_count.
    ///
    /// BBR tracks time for the BBR.BtlBw filter window using a virtual time
    /// tracked by BBR.round_countt, a count of "packet-timed" round-trips.
    /// The BBR.round_count counts packet-timed round trips by recording state
    /// about a sentinel packet, and waiting for an ACK of any data packet that
    /// was sent after that sentinel packet.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.1.1.3.
    fn update_round(&mut self) {
        if self.ack_state.packet_delivered >= self.round.next_round_delivered {
            self.round.next_round_delivered = self.delivery_rate_estimator.delivered();
            self.round.round_count += 1;
            self.round.is_round_start = true;
            // After one round-trip in Fast Recovery, exit the packet conservation mode.
            self.packet_conservation = false;
        } else {
            self.round.is_round_start = false;
        }
    }

    /// Is pipe filled.
    pub fn is_filled_pipe(&self) -> bool {
        self.full_pipe.is_filled_pipe
    }

    /// Is round start.
    pub fn is_round_start(&self) -> bool {
        self.round.is_round_start
    }

    /// Try to update the pacing rate using the given pacing_gain
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.2.1.
    fn set_pacing_rate_with_gain(&mut self, pacing_gain: f64) {
        let rate = (pacing_gain * self.btlbw as f64) as u32;

        // On each data ACK BBR updates its pacing rate to be proportional to
        // BBR.BtlBw, as long as it estimates that it has filled the pipe, or
        // doing so increases the pacing rate.
        if self.is_filled_pipe() || rate > self.pacing_rate {
            self.pacing_rate = rate;
        }
    }

    /// In Drain, BBR aims to quickly drain any queue created in Startup by
    /// switching to a pacing_gain well below 1.0.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.3.3.
    fn enter_drain(&mut self) {
        self.state = BbrStateMachine::Drain;

        // It uses a pacing_gain that is the inverse of the value used during
        // Startup, which drains the queue in one round.
        self.pacing_gain = 1.0 / HIGH_GAIN; // pace slowly
        self.cwnd_gain = HIGH_GAIN; // maintain cwnd
    }

    /// Calculate the target cwnd, which is the upper bound on the volume of data BBR
    /// allows in flight.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.2.3.2 Target cwnd.
    fn inflight(&self, gain: f64) -> u32 {
        if self.rtprop == Duration::MAX {
            // no valid RTT samples yet
            return self.config.initial_cwnd;
        }

        // The "quanta" term allows enough quanta in flight on the sending
        // and receiving hosts to reach full utilization even in high-throughput
        // environments using offloading mechanisms.
        let quanta = 3 * self.send_quantum;

        // The "estimated_bdp" term allows enough packets in flight to fully
        // utilize the estimated BDP of the path, by allowing the flow to send
        // at BBR.BtlBw for a duration of BBR.RTprop.
        let estimated_bdp = self.btlbw as f64 * self.rtprop.as_secs_f64();

        // Scaling up the BDP by cwnd_gain, selected by the BBR state machine to
        // be above 1.0 at all times, bounds in-flight data to a small multiple
        // of the BDP, in order to handle common network and receiver pathologies,
        // such as delayed, stretched, or aggregated ACKs.
        (gain * estimated_bdp) as u32 + quanta
    }

    /// On each ACK, BBR calculates the BBR.target_cwnd.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.2.3.2.
    fn update_target_cwnd(&mut self) {
        self.target_cwnd = self.inflight(self.cwnd_gain);
    }

    /// Check and try to enter or leave Drain state.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.3.3.
    fn check_drain(&mut self, bytes_in_flight: u32, now: Instant) {
        // In Startup, when the BBR "full pipe" estimator estimates that BBR has
        // filled the pipe, BBR switches to its Drain state.
        if self.state == BbrStateMachine::Startup && self.is_filled_pipe() {
            self.enter_drain();
        }

        // In Drain, when the number of packets in flight matches the estimated
        // BDP, meaning BBR estimates that the queue has been fully drained but
        // the pipe is still full, then BBR leaves Drain and enters ProbeBW.
        if self.state == BbrStateMachine::Drain && bytes_in_flight <= self.inflight(1.0) {
            // we estimate queue is drained
            self.enter_probe_bw(now);
        }
    }

    /// Enter the ProbeBW state.
    /// BBR flows spend the vast majority of their time in ProbeBW state,
    /// probing for bandwidth using an approach called gain cycling, which
    /// helps BBR flows reach high throughput, low queuing delay, and
    /// convergence to a fair share of bandwidth.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.3.4.3.
    fn enter_probe_bw(&mut self, now: Instant) {
        self.state = BbrStateMachine::ProbeBW;
        self.pacing_gain = 1.0;
        self.cwnd_gain = self.config.probe_bw_cwnd_gain;

        // Gain Cycling Randomization.
        // To improve mixing and fairness, and to reduce queues when multiple
        // BBR flows share a bottleneck, BBR randomizes the phases of ProbeBW
        // gain cycling by randomly picking an initial phase, from among all but
        // the 3/4 phase, when entering ProbeBW.
        self.cycle_index = GAIN_CYCLE_LEN - 1 - rand::thread_rng().gen_range(0..GAIN_CYCLE_LEN - 1);
        self.advance_cycle_phase(now);
    }

    /// Check if it's time to advance to the next gain cycle phase.
    fn check_cycle_phase(&mut self, now: Instant) {
        if self.state == BbrStateMachine::ProbeBW && self.is_next_cycle_phase(now) {
            self.advance_cycle_phase(now);
        }
    }

    /// Advance cycle phase during ProbeBW state.
    fn advance_cycle_phase(&mut self, now: Instant) {
        // BBR flows spend the vast majority of their time in ProbeBW state,
        // probing for bandwidth using an approach called gain cycling, which
        // helps BBR flows reach high throughput, low queuing delay, and
        // convergence to a fair share of bandwidth.
        self.cycle_stamp = now;
        self.cycle_index = (self.cycle_index + 1) % GAIN_CYCLE_LEN;
        self.pacing_gain = PACING_GAIN_CYCLE[self.cycle_index];
    }

    /// Check if it's time to advance to the next gain cycle phase in ProbeBW state.
    fn is_next_cycle_phase(&mut self, now: Instant) -> bool {
        // Each cycle phase normally lasts for roughly BBR.RTprop.
        let is_full_length = now.saturating_duration_since(self.cycle_stamp) > self.rtprop;

        if self.pacing_gain > 1.0 {
            // Cycle gain = 5/4.
            // It does this until the elapsed time in the phase has
            // been at least BBR.RTprop and either inflight has reached
            // 5/4 * estimated_BDP (which may take longer than BBR.RTprop
            // if BBR.RTprop is low) or some packets have been lost.
            return is_full_length
                && (self.ack_state.newly_lost_bytes > 0
                    || self.ack_state.prior_bytes_in_flight >= self.inflight(self.pacing_gain));
        } else if self.pacing_gain < 1.0 {
            // Cycle gain = 3/4.
            // This phase lasts until either a full BBR.RTprop has elapsed or
            // inflight drops below estimated_BDP.
            return is_full_length || self.ack_state.prior_bytes_in_flight <= self.inflight(1.0);
        }

        // Cycle gain = 1.0, which lasts for roughly BBR.RTprop.
        is_full_length
    }

    /// When restarting from idle, BBR leaves its cwnd as-is and paces
    /// packets at exactly BBR.BtlBw, aiming to return as quickly as possible
    /// to its target operating point of rate balance and a full pipe.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.3.4.4.
    fn handle_restart_from_idle(&mut self, bytes_in_flight: u32) {
        // If the flow's BBR.state is ProbeBW, and the flow is
        // application-limited, and there are no packets in flight currently,
        // then at the moment the flow sends one or more packets BBR sets
        // BBR.pacing_rate to exactly BBR.BtlBw.
        if bytes_in_flight == 0 && self.delivery_rate_estimator.is_app_limited() {
            self.is_idle_restart = true;

            if self.state == BbrStateMachine::ProbeBW {
                self.set_pacing_rate_with_gain(1.0);
            }
        }
    }

    /// Remember cwnd.
    ///
    /// It helps remember and restore the last-known good cwnd (the latest cwnd
    /// unmodulated by loss recovery or ProbeRTT)
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.2.3.4.
    fn save_cwnd(&mut self) {
        self.prior_cwnd = if !self.in_recovery && self.state != BbrStateMachine::ProbeRTT {
            self.cwnd.get()
        } else {
            self.cwnd.get().max(self.prior_cwnd)
        }
    }

    /// Restore cwnd.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.2.3.4.
    fn restore_cwnd(&mut self) {
        self.cwnd.set(self.cwnd.get().max(self.prior_cwnd))
    }

    /// Return cwnd for ProbeRTT state.
    fn probe_rtt_cwnd(&self) -> u32 {
        if self.config.probe_rtt_based_on_bdp {
            return self.inflight(self.config.probe_rtt_cwnd_gain);
        }

        self.config.min_cwnd
    }

    /// Check and try to enter or leave ProbeRTT state.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.3.5.
    fn check_probe_rtt(&mut self, now: Instant, bytes_in_flight: u32) {
        // In any state other than ProbeRTT itself, if the RTProp estimate has
        // not been updated (i.e., by getting a lower RTT measurement) for more
        // than ProbeRTTInterval = 10 seconds, then BBR enters ProbeRTT and
        // reduces the cwnd to a minimal value, BBRMinPipeCwnd (four packets).
        if self.state != BbrStateMachine::ProbeRTT && self.is_rtprop_expired && !self.is_idle_restart {
            self.enter_probe_rtt();

            // Remember the last-known good cwnd and restore it when exiting probe-rtt.
            self.save_cwnd();
            self.probe_rtt_done_stamp = None;
        }

        if self.state == BbrStateMachine::ProbeRTT {
            self.handle_probe_rtt(now, bytes_in_flight);
        }

        self.is_idle_restart = false;
    }

    /// Enter the ProbeRTT state
    fn enter_probe_rtt(&mut self) {
        self.state = BbrStateMachine::ProbeRTT;

        self.pacing_gain = 1.0;
        self.cwnd_gain = 1.0;
    }

    /// Process for the ProbeRTT state
    fn handle_probe_rtt(&mut self, now: Instant, bytes_in_flight: u32) {
        // Ignore low rate samples during ProbeRTT. MarkConnectionAppLimited.
        // C.app_limited = (BW.delivered + packets_in_flight) ? : 1
        self.delivery_rate_estimator.set_app_limited(true);

        if let Some(probe_rtt_done_stamp) = self.probe_rtt_done_stamp {
            if self.is_round_start() {
                self.probe_rtt_round_done = true;
            }

            // After maintaining BBRMinPipeCwnd or fewer packets in flight for
            // at least ProbeRTTDuration (200 ms) and one round trip, BBR leaves
            // ProbeRTT.
            if self.probe_rtt_round_done && now >= probe_rtt_done_stamp {
                self.rtprop_stamp = now;
                self.restore_cwnd();
                self.exit_probe_rtt(now);
            }
        } else if bytes_in_flight <= self.probe_rtt_cwnd() {
            self.probe_rtt_done_stamp = Some(now + self.config.probe_rtt_duration);
            // ProbeRTT round passed.
            self.probe_rtt_round_done = false;
            self.round.next_round_delivered = self.delivery_rate_estimator.delivered();
        }
    }

    /// BBR leaves ProbeRTT and transitions to either Startup or ProbeBW,
    /// depending on whether it estimates the pipe was filled already.
    fn exit_probe_rtt(&mut self, now: Instant) {
        if self.is_filled_pipe() {
            self.enter_probe_bw(now);
        } else {
            self.enter_startup();
        }
    }

    /// On every ACK, the BBR updates its network path model and state machine
    fn update_model_and_state(&mut self, now: Instant) {
        self.update_btlbw();
        self.check_cycle_phase(now);
        self.check_full_pipe();
        self.check_drain(self.stats.bytes_in_flight, now);
        self.update_rtprop(now);
        self.check_probe_rtt(now, self.stats.bytes_in_flight);
    }

    /// BBR adjusts its control parameters to adapt to the updated model.
    fn update_control_parameters(&mut self) {
        self.set_pacing_rate();
        self.set_send_quantum();
        self.set_cwnd();
    }

    /// For every ACK that acknowledges some data packets as delivered, BBR
    /// update the BBR.BtlBw estimator as follows.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.1.1.5.
    fn update_btlbw(&mut self) {
        self.update_round();

        if self.delivery_rate_estimator.delivery_rate() >= self.btlbw
            || !self.delivery_rate_estimator.is_sample_app_limited()
        {
            self.btlbwfilter
                .update_max(self.round.round_count, self.delivery_rate_estimator.delivery_rate());
            self.btlbw = self.btlbwfilter.get();
        }
    }

    /// On every ACK that provides an RTT sample BBR updates the BBR.RTprop
    /// estimator as follows.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.1.2.3.
    fn update_rtprop(&mut self, now: Instant) {
        let sample_rtt = self.delivery_rate_estimator.sample_rtt();

        self.is_rtprop_expired = now.saturating_duration_since(self.rtprop_stamp) > self.config.rtprop_filter_len;

        // Use the same state to track BBR.RTprop and ProbeRTT timing.
        // In section-4.1.2.3, a zero packet.rtt is allowed, but it makes no sense.
        if !sample_rtt.is_zero() && (sample_rtt <= self.rtprop || self.is_rtprop_expired) {
            self.rtprop = sample_rtt;
            self.rtprop_stamp = now;
        }
    }

    /// BBR updates the pacing rate on each ACK as follows.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.2.1.
    fn set_pacing_rate(&mut self) {
        self.set_pacing_rate_with_gain(self.pacing_gain);
    }

    /// On each ACK, BBR runs BBRSetSendQuantum() to update BBR.send_quantum
    /// as follows.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.2.2.
    fn set_send_quantum(&mut self) {
        // A BBR implementation MAY use alternate approaches to select a
        // BBR.send_quantum, as appropriate for the CPU overheads anticipated
        // for senders and receivers, and buffering considerations anticipated
        // in the network path. However, for the sake of the network and other
        // users, a BBR implementation SHOULD attempt to use the smallest
        // feasible quanta.
        // Adjust according to draft-cardwell-iccrg-bbr-congestion-control-02
        let floor = if self.pacing_rate < SEND_QUANTUM_THRESHOLD_PACING_RATE {
            self.config.max_datagram_size
        } else {
            2 * self.config.max_datagram_size
        };

        // BBR.send_quantum = min(BBR.pacing_rate * 1ms, 64KBytes)
        // BBR.send_quantum = max(BBR.send_quantum, floor)
        self.send_quantum = (self.pacing_rate / 1000).clamp(floor, 64 * 1024);
    }

    /// Upon every ACK in Fast Recovery, run the following steps, which help
    /// ensure packet conservation on the first round of recovery, and sending
    /// at no more than twice the current delivery rate on later rounds of
    /// recovery.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.2.3.4.
    fn modulate_cwnd_for_recovery(&mut self, bytes_in_flight: u32) {
        if self.ack_state.newly_lost_bytes > 0 {
            self.cwnd.set(
                self.cwnd
                    .get()
                    .saturating_sub(self.ack_state.newly_lost_bytes)
                    .max(self.config.min_cwnd),
            );
        }

        if self.packet_conservation {
            self.cwnd
                .set(self.cwnd.get().max(bytes_in_flight + self.ack_state.newly_acked_bytes));
        }
    }

    /// To quickly reduce the volume of in-flight data and drain the bottleneck
    /// queue, thereby allowing measurement of BBR.RTprop, BBR bounds the cwnd
    /// to BBRMinPipeCwnd, the minimal value that allows pipelining.
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.2.3.5.
    fn modulate_cwnd_for_probe_rtt(&mut self) {
        // BBR bounds the cwnd in ProbeRTT.
        if self.state == BbrStateMachine::ProbeRTT {
            self.cwnd.set(self.probe_rtt_cwnd());
        }
    }

    /// Adjust the congestion window
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.2.3.6
    fn set_cwnd(&mut self) {
        let bytes_in_flight = self.stats.bytes_in_flight;

        self.update_target_cwnd();
        self.modulate_cwnd_for_recovery(bytes_in_flight);

        if !self.packet_conservation {
            // If BBR has measured enough samples to achieve confidence that it
            // has filled the pipe, then it increases its cwnd based on the
            // number of packets delivered, while bounding its cwnd to be no
            // larger than the BBR.target_cwnd adapted to the estimated BDP.
            if self.is_filled_pipe() {
                self.cwnd
                    .set(self.target_cwnd.min(self.cwnd.get() + self.ack_state.newly_acked_bytes));
            } else if self.cwnd.get() < self.target_cwnd
                || self.delivery_rate_estimator.delivered() < self.config.initial_cwnd
            {
                // Otherwise, if the cwnd is below the target, or the sender has
                // marked so little data delivered (less than InitialCwnd) that
                // it does not yet judge its BBR.BtlBw estimate and BBR.target_cwnd
                // as useful, then it increases cwnd without bounding it to be
                // below the target.
                self.cwnd.set(self.cwnd.get() + self.ack_state.newly_acked_bytes);
            }

            // Finally, BBR imposes a floor of BBRMinPipeCwnd in order to allow
            // pipelining even with small BDPs.
            self.cwnd.set(self.cwnd.get().max(self.config.min_cwnd));
        }

        self.modulate_cwnd_for_probe_rtt();
    }

    /// Enter loss recovery
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.2.3.4.
    fn enter_recovery(&mut self, now: Instant) {
        self.save_cwnd();

        self.recovery_epoch_start = Some(now);

        // Upon entering Fast Recovery, set cwnd to the number of packets still
        // in flight (allowing at least one for a fast retransmit):
        self.cwnd
            .set(self.stats.bytes_in_flight + self.ack_state.newly_acked_bytes.max(self.config.max_datagram_size));

        // Note: After one round-trip in Fast Recovery, BBR.packet_conservation
        // will reset to false
        self.packet_conservation = true;
        self.in_recovery = true;

        self.round.next_round_delivered = self.delivery_rate_estimator.delivered();
    }

    /// Exit loss recovery
    ///
    /// See draft-cardwell-iccrg-bbr-congestion-control-00 Section 4.2.3.4.
    fn exit_recovery(&mut self) {
        self.recovery_epoch_start = None;
        self.packet_conservation = false;
        self.in_recovery = false;

        // Upon exiting loss recovery (RTO recovery or Fast Recovery), either by
        // repairing all losses or undoing recovery, BBR restores the best-known
        // cwnd value we had upon entering loss recovery
        self.restore_cwnd();
    }
}

impl CongestionControllerInner for BbrInner {
    fn name(&self) -> &str {
        "BBR"
    }

    fn on_sent(&mut self, packet: &mut SentPacket) {
        self.delivery_rate_estimator
            .on_packet_sent(packet, self.stats.bytes_in_flight, self.stats.bytes_lost_in_total);

        self.handle_restart_from_idle(self.stats.bytes_in_flight);
        self.stats.bytes_in_flight += packet.sent_size as u32;
    }

    fn begin_ack(&mut self, now: Instant) {
        self.ack_state.newly_acked_bytes = 0;
        self.ack_state.newly_lost_bytes = 0;
        self.ack_state.packet_delivered = 0;
        self.ack_state.last_ack_packet_sent_time = now;
        self.ack_state.prior_bytes_in_flight = self.stats.bytes_in_flight;
        self.ack_state.now = now;
    }

    fn on_ack(&mut self, packet: &mut SentPacket) {
        // Update rate sample by each ack packet.
        self.delivery_rate_estimator.update_rate_sample(packet);

        // Update stats.
        self.stats.bytes_in_flight = self.stats.bytes_in_flight.saturating_sub(packet.sent_size as u32);
        self.stats.bytes_acked_in_total = self.stats.bytes_acked_in_total.saturating_add(packet.sent_size as u32);
        if self.in_slow_start() {
            self.stats.bytes_acked_in_slow_start = self
                .stats
                .bytes_acked_in_slow_start
                .saturating_add(packet.sent_size as u32);
        }

        // Update ack state.
        self.ack_state.newly_acked_bytes += packet.sent_size as u32;
        self.ack_state.last_ack_packet_sent_time = packet.time_sent;

        // Only remember the max P.delivered to determine whether a new round starts.
        self.ack_state.packet_delivered = self.ack_state.packet_delivered.max(packet.rate_sample_state.delivered);
    }

    fn end_ack(&mut self) {
        // Generate rate sample.
        self.delivery_rate_estimator.generate_rate_sample();

        // Check if exit recovery
        if self.in_recovery && !self.in_recovery(self.ack_state.last_ack_packet_sent_time) {
            self.exit_recovery();
        }

        // Update model and control parameters.
        self.update_model_and_state(self.ack_state.now);
        self.update_control_parameters();
    }

    fn on_congestion_event(
        &mut self,
        now: Instant,
        packet: &SentPacket,
        in_persistent_congestion: bool,
        lost_bytes: u32,
    ) {
        self.stats.bytes_in_flight = self.stats.bytes_in_flight.saturating_sub(lost_bytes);
        self.stats.bytes_lost_in_total = self.stats.bytes_lost_in_total.saturating_add(lost_bytes);
        self.ack_state.newly_lost_bytes = self.ack_state.newly_lost_bytes.saturating_add(lost_bytes);

        // Refer to <https://www.rfc-editor.org/rfc/rfc9002#section-7.6.2>.
        // When persistent congestion is declared, the sender's congestion
        // window MUST be reduced to the minimum congestion window.
        match in_persistent_congestion {
            true => {
                self.cwnd.set(self.config.min_cwnd);
                self.recovery_epoch_start = None;
            },
            false => {
                if !self.in_recovery && !self.in_recovery(packet.time_sent) {
                    self.enter_recovery(now);
                }
            },
        }
    }

    fn congestion_window(&self) -> u32 {
        self.cwnd.get().max(self.config.min_cwnd)
    }

    fn pacing_rate(&self) -> Option<u32> {
        Some(self.pacing_rate)
    }

    fn initial_window(&self) -> u32 {
        self.config.initial_cwnd
    }

    fn minimal_window(&self) -> u32 {
        self.config.min_cwnd
    }

    fn in_recovery(&self, sent_time: Instant) -> bool {
        self.recovery_epoch_start.is_some_and(|t| sent_time <= t)
    }

    fn in_slow_start(&self) -> bool {
        self.state == BbrStateMachine::Startup
    }

    fn stats(&self) -> &CongestionStats {
        &self.stats
    }
}

//DeliveryRateEstimator

// Copyright (c) 2023 The TQUIC Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// A generic algorithm for a transport protocol sender to estimate the current
// delivery rate of its data on the fly.
//
// See
// <https://datatracker.ietf.org/doc/html/draft-cheng-iccrg-delivery-rate-estimation-02>.

/// Rate sample output.
///
/// See
/// <https://datatracker.ietf.org/doc/html/draft-cheng-iccrg-delivery-rate-estimation-02#section-3.1.3>.
#[derive(Debug, Default)]
struct RateSample {
    /// rs.delivery_rate: The delivery rate sample (in most cases rs.delivered / rs.interval).
    delivery_rate: u32,

    /// rs.is_app_limited: The P.is_app_limited from the most recent packet delivered;
    /// indicates whether the rate sample is application-limited.
    is_app_limited: bool,

    /// rs.interval: The length of the sampling interval.
    interval: Duration,

    /// rs.delivered: The amount of data marked as delivered over the sampling interval.
    delivered: u32,

    /// rs.prior_delivered: The P.delivered count from the most recent packet delivered.
    prior_delivered: u32,

    /// rs.prior_time: The P.delivered_time from the most recent packet delivered.
    prior_time: Option<Instant>,

    /// rs.send_elapsed: Send time interval calculated from the most recent packet delivered.
    send_elapsed: Duration,

    /// rs.ack_elapsed: ACK time interval calculated from the most recent packet delivered.
    ack_elapsed: Duration,

    /// sample rtt.
    rtt: Duration,
}

/// Delivery rate estimator.
///
/// <https://datatracker.ietf.org/doc/html/draft-cheng-iccrg-delivery-rate-estimation-02#section-3.1.1>.
#[derive(Debug)]
struct DeliveryRateEstimator {
    /// C.delivered: The total amount of data (measured in octets or in packets) delivered
    /// so far over the lifetime of the transport connection. This does not include pure ACK packets.
    delivered: u32,

    /// C.delivered_time: The wall clock time when C.delivered was last updated.
    delivered_time: Instant,

    /// C.first_sent_time: If packets are in flight, then this holds the send time of the packet that
    /// was most recently marked as delivered. Else, if the connection was recently idle, then this
    /// holds the send time of most recently sent packet.
    first_sent_time: Instant,

    /// C.app_limited: The index of the last transmitted packet marked as application-limited,
    /// or 0 if the connection is not currently application-limited.
    last_app_limited_pkt_num: u32,

    /// Record largest acked packet number to determine if app-limited state exits.
    largest_acked_pkt_num: u32,

    /// The last sent packet number.
    /// If application-limited occurs, it will be the end of last_app_limited_pkt_num.
    last_sent_pkt_num: u32,

    /// Rate sample.
    rate_sample: RateSample,
}

impl DeliveryRateEstimator {
    /// Upon each packet transmission.
    /// See <https://datatracker.ietf.org/doc/html/draft-cheng-iccrg-delivery-rate-estimation-02#section-3.2>.
    pub fn on_packet_sent(&mut self, packet: &mut SentPacket, bytes_in_flight: u32, bytes_lost: u32) {
        // no packets in flight yet?
        if bytes_in_flight == 0 {
            self.first_sent_time = packet.time_sent;
            self.delivered_time = packet.time_sent;
        }

        packet.rate_sample_state.first_sent_time = Some(self.first_sent_time);
        packet.rate_sample_state.delivered_time = Some(self.delivered_time);
        packet.rate_sample_state.delivered = self.delivered;
        packet.rate_sample_state.is_app_limited = self.is_app_limited();
        packet.rate_sample_state.tx_in_flight = bytes_in_flight;
        packet.rate_sample_state.lost = bytes_lost;

        self.last_sent_pkt_num = packet.pkt_num;
    }

    /// Update rate sampler (rs) when a packet is SACKed or ACKed.
    /// See <https://datatracker.ietf.org/doc/html/draft-cheng-iccrg-delivery-rate-estimation-02#section-3.3>.
    pub fn update_rate_sample(&mut self, packet: &mut SentPacket) {
        if packet.rate_sample_state.delivered_time.is_none() || packet.time_acked.is_none() {
            // Packet already SACKed or packet not acked
            return;
        }

        self.delivered = self.delivered.saturating_add(packet.sent_size as u32);
        // note: Update rate sample after P.time_acked got update. The default Instant::now() is
        // not accurate for estimating ack_elapsed.
        self.delivered_time = packet.time_acked.unwrap_or(Instant::now());

        // Update info using the newest packet:
        if self.rate_sample.prior_time.is_none()
            || packet.rate_sample_state.delivered > self.rate_sample.prior_delivered
        {
            self.rate_sample.prior_delivered = packet.rate_sample_state.delivered;
            self.rate_sample.prior_time = packet.rate_sample_state.delivered_time;
            self.rate_sample.is_app_limited = packet.rate_sample_state.is_app_limited;

            self.first_sent_time = packet.time_sent;
        }

        // Use each ACK to update delivery rate.
        self.rate_sample.send_elapsed = packet
            .time_sent
            .saturating_duration_since(packet.rate_sample_state.first_sent_time.unwrap_or(packet.time_sent));
        self.rate_sample.ack_elapsed = self
            .delivered_time
            .saturating_duration_since(packet.rate_sample_state.delivered_time.unwrap_or(packet.time_sent));
        self.rate_sample.rtt = self.delivered_time.saturating_duration_since(packet.time_sent);

        self.rate_sample.delivered = self.delivered.saturating_sub(self.rate_sample.prior_delivered);

        // Mark the packet as delivered once it's SACKed to
        // avoid being used again when it's cumulatively acked.
        packet.rate_sample_state.delivered_time = None;

        self.largest_acked_pkt_num = packet.pkt_num.max(self.largest_acked_pkt_num);
    }

    /// Upon receiving ACK, fill in delivery rate sample rs.
    /// See <https://datatracker.ietf.org/doc/html/draft-cheng-iccrg-delivery-rate-estimation-02#section-3.3>.
    pub fn generate_rate_sample(&mut self) {
        // For each newly SACKed or ACKed packet P,
        //     `UpdateRateSample(P, rs)`
        // It's done before generate_rate_sample is called.

        // Clear app-limited field if bubble is ACKed and gone.
        if self.is_app_limited() && self.largest_acked_pkt_num > self.last_app_limited_pkt_num {
            self.set_app_limited(false);
        }

        // Nothing delivered on this ACK.
        if self.rate_sample.prior_time.is_none() {
            return;
        }

        // Use the longer of the send_elapsed and ack_elapsed.
        self.rate_sample.interval = self.rate_sample.send_elapsed.max(self.rate_sample.ack_elapsed);

        self.rate_sample.delivered = self.delivered.saturating_sub(self.rate_sample.prior_delivered);

        if self.rate_sample.interval.is_zero() {
            return;
        }

        self.rate_sample.delivery_rate =
            self.rate_sample.delivered * 1_000_000_u32 / self.rate_sample.interval.as_micros() as u32;
    }

    /// Set app limited status and record the latest packet num as end of app limited mode.
    pub fn set_app_limited(&mut self, is_app_limited: bool) {
        self.last_app_limited_pkt_num = if is_app_limited {
            self.last_sent_pkt_num.max(1)
        } else {
            0
        }
    }

    /// Check if application limited.
    /// See <https://datatracker.ietf.org/doc/html/draft-cheng-iccrg-delivery-rate-estimation-02#section-3.4>.
    pub fn is_app_limited(&self) -> bool {
        self.last_app_limited_pkt_num != 0
    }

    /// C.delivered.
    pub fn delivered(&self) -> u32 {
        self.delivered
    }

    /// rs.delivered.
    pub fn sample_delivered(&self) -> u32 {
        self.rate_sample.delivered
    }

    /// rs.prior_delivered.
    pub fn sample_prior_delivered(&self) -> u32 {
        self.rate_sample.prior_delivered
    }

    /// Delivery rate.
    pub fn delivery_rate(&self) -> u32 {
        self.rate_sample.delivery_rate
    }

    /// Get rate sample rtt.
    pub fn sample_rtt(&self) -> Duration {
        self.rate_sample.rtt
    }

    /// Check whether the current rate sample is application limited.
    pub fn is_sample_app_limited(&self) -> bool {
        self.rate_sample.is_app_limited
    }
}

impl Default for DeliveryRateEstimator {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            delivered: 0,
            delivered_time: now,
            first_sent_time: now,
            last_app_limited_pkt_num: 0,
            largest_acked_pkt_num: 0,
            last_sent_pkt_num: 0,
            rate_sample: RateSample::default(),
        }
    }
}

//MinMax

// Copyright (c) 2023 The TQUIC Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

/*
 * Copyright 2017, Google Inc.
 *
 * Use of this source code is governed by the following BSD-style license:
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are
 * met:
 *
 *    * Redistributions of source code must retain the above copyright
 * notice, this list of conditions and the following disclaimer.
 *    * Redistributions in binary form must reproduce the above
 * copyright notice, this list of conditions and the following disclaimer
 * in the documentation and/or other materials provided with the
 * distribution.
 *
 *    * Neither the name of Google Inc. nor the names of its
 * contributors may be used to endorse or promote products derived from
 * this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 * "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 * LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
 * A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
 * OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
 * LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
 * OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

// A windowed min/max estimator, which is based on an algorithm by Kathleen Nichols.
// Refer to <https://groups.google.com/g/bbr-dev/c/3RTgkzi5ZD8>.
//  * lib/minmax.c: windowed min/max tracker
//
// Kathleen Nichols' algorithm for tracking the minimum (or maximum)
// value of a data stream over some fixed time interval.  (E.g.,
// the minimum RTT over the past five minutes.) It uses constant
// space and constant time per update yet almost always delivers
// the same minimum as an implementation that has to keep all the
// data in the window.
//
// The algorithm keeps track of the best, 2nd best & 3rd best min
// values, maintaining an invariant that the measurement time of
// the n'th best >= n-1'th best. It also makes sure that the three
// values are widely separated in the time window since that bounds
// the worse case error when that data is monotonically increasing
// over the window.
//
// Upon getting a new min, we can forget everything earlier because
// it has no value - the new min is <= everything else in the window
// by definition and it's the most recent. So we restart fresh on
// every new min and overwrites 2nd & 3rd choices. The same property
// holds for 2nd & 3rd best.

#[derive(Debug, Copy, Clone, Default)]
struct MinMaxSample {
    /// Round trip count.
    time: u32,

    /// Sample value.
    value: u32,
}

#[derive(Debug)]
struct MinMax {
    /// The max lasting time window to pick up the best sample.
    window: u32,

    /// The best, second best, third best samples.
    samples: [MinMaxSample; 3],
}

impl MinMax {
    pub fn new(window: u32) -> Self {
        Self {
            window,
            samples: [Default::default(); 3],
        }
    }

    /// Set window size.
    pub fn set_window(&mut self, window: u32) {
        self.window = window;
    }

    /// Reset all samples to the given sample.
    pub fn reset(&mut self, sample: MinMaxSample) {
        self.samples.fill(sample)
    }

    /// As time advances, update the 1st, 2nd, and 3rd choices.
    fn subwin_update(&mut self, sample: MinMaxSample) {
        let dt = sample.time.saturating_sub(self.samples[0].time);
        if dt > self.window {
            // Passed entire window without a new sample so make 2nd
            // choice the new sample & 3rd choice the new 2nd choice.
            // we may have to iterate this since our 2nd choice
            // may also be outside the window (we checked on entry
            // that the third choice was in the window).
            self.samples[0] = self.samples[1];
            self.samples[1] = self.samples[2];
            self.samples[2] = sample;
            if sample.time.saturating_sub(self.samples[0].time) > self.window {
                self.samples[0] = self.samples[1];
                self.samples[1] = self.samples[2];
                self.samples[2] = sample;
            }
        } else if self.samples[1].time == self.samples[0].time && dt > self.window / 4_u32 {
            // We've passed a quarter of the window without a new sample
            // so take a 2nd choice from the 2nd quarter of the window.
            self.samples[2] = sample;
            self.samples[1] = sample;
        } else if self.samples[2].time == self.samples[1].time && dt > self.window / 2_u32 {
            // We've passed half the window without finding a new sample
            // so take a 3rd choice from the last half of the window
            self.samples[2] = sample;
        }
    }

    /// Check if new measurement updates the 1st, 2nd or 3rd choice max.
    pub fn update_max(&mut self, time: u32, value: u32) {
        if time < self.samples[2].time {
            // Time should be monotonically increasing.
            return;
        }

        let sample = MinMaxSample { time, value };

        if self.samples[0].value == 0  // uninitialized
            || sample.value >= self.samples[0].value // found new max?
            || sample.time.saturating_sub(self.samples[2].time) > self.window
        // nothing left in window?
        {
            self.reset(sample); // forget earlier samples
            return;
        }

        if sample.value >= self.samples[1].value {
            self.samples[2] = sample;
            self.samples[1] = sample;
        } else if sample.value >= self.samples[2].value {
            self.samples[2] = sample;
        }

        self.subwin_update(sample);
    }

    /// Check if new measurement updates the 1st, 2nd or 3rd choice min.
    pub fn update_min(&mut self, time: u32, value: u32) {
        if time < self.samples[2].time {
            // Time should be monotonically increasing.
            return;
        }

        let sample = MinMaxSample { time, value };

        if self.samples[0].value == 0  // uninitialised
            || sample.value <= self.samples[0].value // found new min?
            || sample.time.saturating_sub(self.samples[2].time) > self.window
        // nothing left in window?
        {
            self.reset(sample); // forget earlier samples
            return;
        }

        if sample.value <= self.samples[1].value {
            self.samples[2] = sample;
            self.samples[1] = sample;
        } else if sample.value <= self.samples[2].value {
            self.samples[2] = sample;
        }

        self.subwin_update(sample);
    }

    /// Get the min/max value.
    pub fn get(&self) -> u32 {
        self.samples[0].value
    }
}

impl Default for MinMax {
    fn default() -> Self {
        Self {
            // The default window for BBR is 10 round trips
            window: 10,
            samples: [Default::default(); 3],
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn minmax_update_max() {
        let mut min_max = MinMax::new(15);
        let round: u32 = 20;

        min_max.set_window(10);
        assert_eq!(min_max.window, 10);

        // Uninitialised.
        min_max.update_max(1, 200);
        assert_eq!(min_max.get(), 200);
        // Nothing left in window.
        min_max.update_max(round, 120);
        assert_eq!(min_max.get(), 120);
        // Found new max.
        min_max.update_max(round + 1, 150);
        assert_eq!(min_max.get(), 150);
        // Validate sample: time should be increasing.
        min_max.update_max(round, 180);
        assert_eq!(min_max.get(), 150);
        // Duration is in (0, window/4], do nothing.
        min_max.update_max(round + 2, 120);
        assert_eq!(min_max.get(), 150);
        // Duration is in (window/4, window/2), update sample 1 and sample 2.
        min_max.update_max(round + 4, 110);
        assert_eq!(min_max.get(), 150);
        // Duration between sample and sample 0 and sample 1 are both larger than window.
        min_max.update_max(round + 8, 100);
        assert_eq!(min_max.get(), 150);
        // Update sample 3.
        min_max.update_max(round + 9, 105);
        assert_eq!(min_max.get(), 150);
        assert_eq!(min_max.samples[1].value, 110);
        assert_eq!(min_max.samples[2].value, 105);
        min_max.update_max(round + 15, 90);
        assert_eq!(min_max.get(), 105);
        // Merge and update sample 2 and sample 3.
        min_max.update_max(round + 17, 95);
        assert_eq!(min_max.get(), 105);
        assert_eq!(min_max.samples[1].value, 95);
        assert_eq!(min_max.samples[2].value, 95);
    }

    #[test]
    fn minmax_update_min() {
        let mut min_max = MinMax::default();
        assert_eq!(min_max.window, 10);

        let round: u32 = 20;
        // Uninitialised.
        min_max.update_min(1, 100);
        assert_eq!(min_max.get(), 100);
        // Nothing left in window.
        min_max.update_min(round, 120);
        assert_eq!(min_max.get(), 120);
        // Found new min.
        min_max.update_min(round + 1, 110);
        assert_eq!(min_max.get(), 110);
        // Validate sample: time should be increasing.
        min_max.update_min(round, 90);
        assert_eq!(min_max.get(), 110);
        // Update sample 2 and sample 3.
        min_max.update_min(round + 4, 120);
        assert_eq!(min_max.get(), 110);
        min_max.update_min(round + 8, 115);
        assert_eq!(min_max.samples[1].value, 115);
        min_max.update_min(round + 9, 120);
        assert_eq!(min_max.samples[2].value, 120);
        min_max.update_min(round + 10, 118);
        assert_eq!(min_max.samples[2].value, 118);
    }
}

//use super::congestion_control::{CongestionController, CongestionStats};
use std::fmt;

use crate::{
    inetstack::protocols::tcp::SeqNumber,
    runtime::watched::WatchedValue,
};

/// Congestion control statistics.
#[derive(Debug, Default, Clone)]
struct CongestionStats {
    /// Bytes in flight.
    pub bytes_in_flight: u32,

    /// Total bytes sent in slow start.
    pub bytes_sent_in_slow_start: u32,

    /// Total bytes acked in slow start.
    pub bytes_acked_in_slow_start: u32,

    /// Total bytes lost in slow start.
    pub bytes_lost_in_slow_start: u32,

    /// Total bytes sent.
    pub bytes_sent_in_total: u32,

    /// Total bytes acked.
    pub bytes_acked_in_total: u32,

    /// Total bytes lost.
    pub bytes_lost_in_total: u32,
}

/// Congestion control interfaces shared by different algorithms.
trait CongestionControllerInner {
    /// Name of congestion control algorithm.
    fn name(&self) -> &str;

    /// Callback after packet was sent out.
    fn on_sent(&mut self, packet: &mut SentPacket);

    /// Callback for ack packets preprocessing.
    fn begin_ack(&mut self, now: Instant);

    /// Callback for processing each ack packet.
    fn on_ack(&mut self, packet: &mut SentPacket);

    /// Callback for Updating states after all ack packets are processed.
    fn end_ack(&mut self);

    /// Congestion event.
    fn on_congestion_event(
        &mut self,
        now: Instant,
        packet: &SentPacket,
        is_persistent_congestion: bool,
        lost_bytes: u32,
    );

    /// Check if in slow start.
    fn in_slow_start(&self) -> bool {
        true
    }

    /// Check if in recovery mode.
    fn in_recovery(&self, sent_time: Instant) -> bool {
        false
    }

    /// Current congestion window.
    fn congestion_window(&self) -> u32;

    /// Current pacing rate estimated by Congestion Control Algorithm (CCA).
    /// If CCA does not estimate pacing rate, return None.
    fn pacing_rate(&self) -> Option<u32> {
        None
    }

    /// Initial congestion window.
    fn initial_window(&self) -> u32;

    /// Minimal congestion window.
    fn minimal_window(&self) -> u32;

    /// Congestion stats.
    fn stats(&self) -> &CongestionStats;
}

impl fmt::Debug for dyn CongestionControllerInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "congestion controller.")
    }
}

//use crate::space::SentPacket;

/// Metadata of sent packet
#[derive(Clone, Debug)]
struct SentPacket {
    /// The packet type of the sent packet.
    //pub pkt_type: packet::PacketType,

    /// The packet number of the sent packet.
    pub pkt_num: u32,

    pub ack_no: u32,

    /// The Frames metadata of the sent packet.
    //pub frames: Vec<frame::Frame>,

    /// The time the packet was sent.
    pub time_sent: Instant,

    /// The time the packet was acknowledged, if any.
    pub time_acked: Option<Instant>,

    /// The time the packet was declared lost, if any.
    //pub time_lost: Option<Instant>,

    /// A Boolean that indicates whether a packet is ack-eliciting. If true, it
    /// is expected that an acknowledgment will be received, though the peer
    /// could delay sending the ACK frame containing it by up to the max_ack_delay.
    //pub ack_eliciting: bool,

    /// A Boolean that indicates whether the packet counts toward bytes in
    /// flight.
    //pub in_flight: bool,

    /// Whether the packet contains CRYPTO or STREAM frame
    //pub has_data: bool,

    /// Whether it is a PMUT probe packet
    //pub pmtu_probe: bool,

    /// Whether it consumes the pacer's tokens
    //pub pacing: bool,

    /// The number of bytes sent in the packet, not including UDP or IP overhead,
    /// but including QUIC framing overhead.
    pub sent_size: usize,

    /// Snapshot of the current delivery information.
    pub rate_sample_state: RateSamplePacketState,
    // Status about buffered frames written into the packet.
    //pub buffer_flags: BufferFlags,
}

/// Per-packet state for delivery rate estimation.
/// See https://datatracker.ietf.org/doc/html/draft-cheng-iccrg-delivery-rate-estimation-02#section-3.1.2
#[derive(Debug, Default, Clone)]
struct RateSamplePacketState {
    /// P.delivered: C.delivered when the packet was sent from transport connection C.
    pub delivered: u32,

    /// P.delivered_time: C.delivered_time when the packet was sent.
    pub delivered_time: Option<Instant>,

    /// P.first_sent_time: C.first_sent_time when the packet was sent.
    pub first_sent_time: Option<Instant>,

    /// P.is_app_limited: true if C.app_limited was non-zero when the packet was sent, else false.
    pub is_app_limited: bool,

    /// packet.tx_in_flight: The volume of data that was estimated to be in flight at the time of the transmission of the packet.
    pub tx_in_flight: u32,

    /// packet.lost: The volume of data that was declared lost on transmission.
    pub lost: u32,
}

#[derive(Debug)]
pub struct Bbr {
    inner: RefCell<BbrInner>,
    next_send_seq: Cell<SeqNumber>,
    pkts: RefCell<std::collections::VecDeque<SentPacket>>,
    cwnd: Rc<WatchedValue<u32>>,
}

impl SlowStartCongestionAvoidance for Bbr {
    fn get_cwnd(&self) -> u32 {
        self.cwnd.get()
    }

    fn watch_cwnd(&self) -> (u32, crate::runtime::watched::WatchFuture<'_, u32>) {
        self.cwnd.watch()
    }

    // Called immediately before the cwnd check is performed before data is sent.
    fn on_cwnd_check_before_send(&self) {}

    fn on_ack_received(
        &self,
        _rto: Duration,
        _send_unacked: SeqNumber,
        _send_next: SeqNumber,
        _ack_seq_no: SeqNumber,
        seg_len: u32,
    ) {
        let mut inner = self.inner.borrow_mut();
        inner.begin_ack(Instant::now());
        let mut pkts = self.pkts.borrow_mut();
        loop {
            if let Some(p) = pkts.front_mut() {
                if p.ack_no != _ack_seq_no.into() {
                    inner.on_ack(p);
                    pkts.pop_front();
                    continue;
                }
            }
            break;
        }
        inner.end_ack();
    }

    // Called immediately before retransmit after RTO.
    fn on_rto(&self, _send_unacked: SeqNumber) {}

    // Called immediately before a segment is sent for the 1st time.
    fn on_send(&self, _rto: Duration, _num_sent_bytes: u32) {
        let head: u32 = self.next_send_seq.get().into();
        let tail = head + _num_sent_bytes;
        self.next_send_seq.set(SeqNumber::from(tail));
        let mut pkt = SentPacket {
            pkt_num: head,
            time_sent: Instant::now(),
            time_acked: None,
            sent_size: _num_sent_bytes as usize,
            rate_sample_state: RateSamplePacketState::default(),
            ack_no: tail,
        };
        self.inner.borrow_mut().on_sent(&mut pkt);
        self.pkts.borrow_mut().push_back(pkt);
    }
}

impl FastRetransmitRecovery for Bbr {}
impl LimitedTransmit for Bbr {}

impl CongestionControl for Bbr {
    fn new(mss: usize, seq_no: SeqNumber, options: Option<super::options::Options>) -> Box<dyn CongestionControl>
    where
        Self: Sized {
            let cwnd = Rc::new(WatchedValue::new(0));
            let (inner, cwnd) = BbrInner::new(BbrConfig::default());
            Box::new(Self{
                inner: RefCell::new(inner),
                next_send_seq: Cell::new(seq_no),
                pkts: RefCell::new(VecDeque::new()),
                cwnd,
            })
    }
}
