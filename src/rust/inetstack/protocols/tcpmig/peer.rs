// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::{
    active::ActiveMigration,
    constants::*,
    segment::{
        TcpMigHeader,
        TcpMigSegment,
    },
    ApplicationState,
};
use crate::{
    capy_profile,
    capy_profile_merge_previous,
    capy_time_log,
    inetstack::protocols::{
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        ip::IpProtocol,
        ipv4::Ipv4Header,
        tcp::{
            peer::{
                state::TcpState,
                TcpMigContext,
            },
            segment::TcpHeader,
        },
        tcpmig::segment::MigrationStage,
        udp::{
            UdpDatagram,
            UdpHeader,
        },
    },
    runtime::{
        fail::Fail,
        memory::{
            Buffer,
            DataBuffer,
        },
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
    },
    QDesc,
};

use ::std::{
    collections::HashMap,
    env,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    rc::Rc,
    thread,
};
use std::{
    borrow::BorrowMut,
    cell::{
        Cell,
        OnceCell,
        RefCell,
    },
    collections::hash_map::Entry,
    ffi::c_void,
    time::Instant,
};

#[cfg(feature = "profiler")]
use crate::timer;

use crate::capy_log_mig;

//======================================================================================================================
// Structures
//======================================================================================================================

pub enum TcpmigReceiveStatus {
    Ok,
    SentReject,
    Rejected(SocketAddrV4, SocketAddrV4),
    ReturnedBySwitch(SocketAddrV4, SocketAddrV4),
    PrepareMigrationAcked(QDesc),
    // Some(buf) when ActiveMigration -> TcpMigPeer, None when TcpMigPeer -> TcpPeer
    StateReceived(TcpState, Option<Buffer>),
    MigrationCompleted,

    // Heartbeat protocol.
    HeartbeatResponse(usize),
}

/// TCPMig Peer
pub struct TcpMigPeer {
    /// Underlying runtime.
    rt: Rc<dyn NetworkRuntime>,

    /// Local link address.
    local_link_addr: MacAddress,
    /// Local IPv4 address.
    local_ipv4_addr: Ipv4Addr,

    user_connection: user_connection::Peer,

    /// Connections being actively migrated in/out.
    ///
    /// key = remote.
    active_migrations: HashMap<SocketAddrV4, ActiveMigration>,

    incoming_user_data: HashMap<SocketAddrV4, Buffer>,

    self_udp_port: u16,

    heartbeat_message: Box<TcpMigSegment>,

    /// key: remote addr
    application_state: HashMap<SocketAddrV4, MigratedApplicationState>,

    /// for testing
    additional_mig_delay: u32,
}

#[derive(Default)]
pub enum MigratedApplicationState {
    #[default]
    None,
    Registered(Rc<RefCell<dyn ApplicationState>>),
    MigratedIn(Buffer),
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate functions for [TcpMigPeer].
impl TcpMigPeer {
    /// Creates a TCPMig peer.
    pub fn new(rt: Rc<dyn NetworkRuntime>, local_link_addr: MacAddress, local_ipv4_addr: Ipv4Addr) -> Self {
        log_init();

        Self {
            rt: rt.clone(),
            local_link_addr,
            local_ipv4_addr,
            user_connection: user_connection::Peer::Nop,
            active_migrations: HashMap::new(),
            incoming_user_data: HashMap::new(),
            self_udp_port: SELF_UDP_PORT, // TEMP

            heartbeat_message: Box::new(TcpMigSegment::new(
                Ethernet2Header::new(FRONTEND_MAC, local_link_addr, EtherType2::Ipv4),
                Ipv4Header::new(local_ipv4_addr, FRONTEND_IP, IpProtocol::UDP),
                TcpMigHeader::new(
                    SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0),
                    SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0),
                    4,
                    MigrationStage::HeartbeatUpdate,
                    SELF_UDP_PORT,
                    FRONTEND_PORT,
                ),
                Buffer::Heap(DataBuffer::new(4).unwrap()),
            )),

            application_state: HashMap::new(),

            // for testing
            additional_mig_delay: env::var("MIG_DELAY")
            .unwrap_or_else(|_| String::from("0")) // Default value is 0 if MIG_DELAY is not set
            .parse::<u32>()
            .expect("Invalid DELAY value"),
        }
    }

    pub fn should_migrate(&self) -> bool {
        // if self.mig_off != 0 {
        //     return false;
        // }

        // static mut FLAG: i32 = 0;

        // unsafe {
        //     // if FLAG == 5 {
        //     //     FLAG = 0;
        //     // }
        //     FLAG += 1;
        //     eprintln!("FLAG: {}", FLAG);
        //     FLAG == 30
        // }
        thread_local! {
            static COUNT: Cell<u32> = Cell::new(0);
            static MIGRATE_AFTER: OnceCell<Option<u32>> = OnceCell::new();
        }
        let Some(migrate_after) = MIGRATE_AFTER.with(|migrate_after| {
            migrate_after
                .get_or_init(|| std::env::var("MIG_AFTER").ok().map(|n| n.parse().unwrap()))
                .clone()
        }) else {
            return false;
        };
        let count = COUNT.get();
        COUNT.set(count + 1);
        count == migrate_after
    }

    pub fn set_port(&mut self, port: u16) {
        self.self_udp_port = port;
    }

    /// Consumes the payload from a buffer.
    pub fn receive(
        &mut self,
        ipv4_hdr: &Ipv4Header,
        buf: Buffer,
        ctx: TcpMigContext,
    ) -> Result<TcpmigReceiveStatus, Fail> {
        // Parse header.
        let (hdr, buf) = TcpMigHeader::parse(ipv4_hdr, buf)?;
        capy_log_mig!("\n\n[RX] TCPMig");

        let remote = hdr.client;

        // Heartbeat response from switch.
        if hdr.stage == MigrationStage::HeartbeatResponse {
            let global_queue_len_sum = u32::from_be_bytes(buf[0..4].try_into().unwrap());
            return Ok(TcpmigReceiveStatus::HeartbeatResponse(
                global_queue_len_sum.try_into().expect("heartbeat u32 to usize failed"),
            ));
        }

        // First packet that target receives.
        if hdr.stage == MigrationStage::PrepareMigration {
            // capy_profile!("prepare_ack");

            capy_log_mig!("******* MIGRATION REQUESTED *******");
            capy_log_mig!("PREPARE_MIG {}", remote);
            let target = SocketAddrV4::new(self.local_ipv4_addr, self.self_udp_port);
            capy_log_mig!("I'm target {}", target);

            capy_time_log!("RECV_PREPARE_MIG,({})", remote);

            let active = ActiveMigration::new(
                self.rt.clone(),
                self.local_ipv4_addr,
                self.local_link_addr,
                if self.local_ipv4_addr == FRONTEND_IP {
                    BACKEND_IP
                } else {
                    FRONTEND_IP
                },
                if self.local_link_addr == FRONTEND_MAC {
                    BACKEND_MAC
                } else {
                    FRONTEND_MAC
                },
                self.self_udp_port,
                hdr.origin.port(),
                hdr.origin,
                hdr.client,
                None,
            );
            if let Some(..) = self.active_migrations.insert(remote, active) {
                // todo!("duplicate active migration");
                // It happens when a backend send PREPARE_MIGRATION to the switch
                // but it receives back the message again (i.e., this is the current minimum workload backend)
                // In this case, remove the active migration.
                capy_log_mig!("It returned back to itself, maybe it's the current-min-workload server");
                self.active_migrations.remove(&remote);
                return Ok(TcpmigReceiveStatus::ReturnedBySwitch(hdr.origin, hdr.client));
            }
        }

        let mut entry = match self.active_migrations.entry(remote) {
            Entry::Vacant(..) => panic!("no such active migration: {:#?}", hdr),
            Entry::Occupied(entry) => entry,
        };
        let active = entry.get_mut();

        capy_log_mig!("Active migration {:?}", remote);
        let mut status = active.process_packet(ipv4_hdr, hdr, buf, ctx)?;
        match &mut status {
            TcpmigReceiveStatus::PrepareMigrationAcked(..) => (),
            TcpmigReceiveStatus::StateReceived(state, data) => {
                // capy_profile_merge_previous!("migrate_ack");

                // Push user data into queue.
                /* if let Some(data) = user_data {
                    assert!(self.incoming_user_data.insert(remote, data).is_none());
                } */

                let (local_addr, remote_addr) = state.connection();
                capy_log_mig!("======= MIGRATING IN STATE ({local_addr}, {remote_addr}) =======");

                match state.app_state {
                    MigratedApplicationState::MigratedIn(..) => {
                        capy_log_mig!("Received app state from migration");
                        self.application_state
                            .insert(remote_addr, std::mem::take(&mut state.app_state));
                    },
                    _ => (),
                }

                // Remove active migration.
                // entry.remove();

                capy_log_mig!("Call user connection migrate_in(..)");
                self.user_connection.migrate_in(remote_addr, data.take().unwrap());
                capy_log_mig!("Call done");
            },
            TcpmigReceiveStatus::MigrationCompleted => {
                // Remove active migration.
                entry.remove();

                // capy_log_mig!("1");
                // capy_log_mig!(
                //     "2, active_migrations: {:?}, removing {}",
                //     self.active_migrations.keys().collect::<Vec<_>>(),
                //     remote
                // );
                capy_log_mig!(
                    "active_migrations: {:?}, removing {}",
                    self.active_migrations.keys().collect::<Vec<_>>(),
                    remote
                );

                //self.is_currently_migrating = false;
                capy_log_mig!("CONN_STATE_ACK ({})\n=======  MIGRATION COMPLETE! =======\n\n", remote);
            },
            TcpmigReceiveStatus::Rejected(..) | TcpmigReceiveStatus::SentReject => {
                // Remove active migration.
                entry.remove();
                capy_log_mig!("Removed rejected active migration: {remote}");
            },
            TcpmigReceiveStatus::Ok => (),
            TcpmigReceiveStatus::HeartbeatResponse(..) => panic!("heartbeat not handled earlier"),
            TcpmigReceiveStatus::ReturnedBySwitch(..) => panic!("ReturnedBySwitch returned by active migration"),
        };

        Ok(status)
    }

    pub fn initiate_migration(&mut self, conn: (SocketAddrV4, SocketAddrV4), qd: QDesc) {
        {
            // capy_profile!("additional_delay");
            for _ in 0..self.additional_mig_delay {
                thread::yield_now();
            }
        }

        let (local, remote) = conn;

        // eprintln!("initiate migration for connection {} <-> {}", origin, client);

        //let origin = SocketAddrV4::new(self.local_ipv4_addr, origin_port);
        // let target = SocketAddrV4::new(FRONTEND_IP, FRONTEND_PORT); // TEMP

        let active = ActiveMigration::new(
            self.rt.clone(),
            self.local_ipv4_addr,
            self.local_link_addr,
            if self.local_ipv4_addr == FRONTEND_IP {
                BACKEND_IP
            } else {
                FRONTEND_IP
            },
            if self.local_link_addr == FRONTEND_MAC {
                BACKEND_MAC
            } else {
                FRONTEND_MAC
            },
            self.self_udp_port,
            10000,
            // if self.self_udp_port == 10001 { 10000 } else { 10001 }, // dest_udp_port is unknown until it receives PREPARE_MIGRATION_ACK, so it's 0 initially.
            local,
            remote,
            Some(qd),
        ); // Inho: Q. Why link_addr (MAC addr) is needed when the libOS has arp_table already? Is it possible to use the arp_table instead?

        let active = match self.active_migrations.entry(remote) {
            Entry::Occupied(..) => panic!("duplicate initiate migration"),
            Entry::Vacant(entry) => entry.insert(active),
        };

        active.initiate_migration();
    }

    pub fn migrate_out_connection_state(&mut self, qd: QDesc, mut state: TcpState) {
        let remote = state.remote();

        match self.application_state.remove(&remote) {
            Some(MigratedApplicationState::Registered(app_state)) => {
                capy_log_mig!("Adding application state");
                state.add_app_state(app_state)
            },
            _ => (),
        }

        let active = self.active_migrations.get_mut(&remote).unwrap();
        let user_state = self.user_connection.migrate_out(qd);
        active.send_connection_state(state, user_state);

        // Remove migrated user data if present.
        self.incoming_user_data.remove(&remote);
    }

    /// Returns the moved buffers for further use by the caller if packet was not buffered.
    pub fn try_buffer_packet(
        &mut self,
        remote: SocketAddrV4,
        tcp_hdr: TcpHeader,
        data: Buffer,
    ) -> Result<(), (TcpHeader, Buffer)> {
        match self.active_migrations.get_mut(&remote) {
            Some(active) => {
                capy_log_mig!("mig_prepared ==> buffer");
                active.buffer_packet(tcp_hdr, data);
                Ok(())
            },
            None => {
                capy_log_mig!("trying to buffer, but there is no corresponding active migration");
                Err((tcp_hdr, data))
            },
        }
    }

    /// Returns the buffered packets for the migrated connection.
    pub fn close_active_migration(&mut self, remote: SocketAddrV4, qd: QDesc) -> Option<Vec<(TcpHeader, Buffer)>> {
        let migration = self
            .active_migrations
            .remove(&remote)
            .map(|mut active| active.take_buffered_packets())?;
        self.user_connection.migration_complete(remote, qd);
        Some(migration)
    }

    pub fn send_heartbeat(&mut self, queue_len: usize) {
        let queue_len: u32 = queue_len.try_into().expect("Queue len bigger than 4 billion");
        self.heartbeat_message.data[0..4].copy_from_slice(&queue_len.to_be_bytes());
        // self.rt.transmit(self.heartbeat_message.clone());
    }

    pub fn register_application_state(&mut self, remote: SocketAddrV4, state: Rc<RefCell<dyn ApplicationState>>) {
        self.application_state
            .insert(remote, MigratedApplicationState::Registered(state));
    }

    pub fn get_migrated_application_state<T: ApplicationState + 'static>(
        &mut self,
        remote: SocketAddrV4,
    ) -> Option<Rc<RefCell<T>>> {
        let state = match self.application_state.remove(&remote) {
            Some(MigratedApplicationState::MigratedIn(state)) => T::deserialize(&state),
            _ => return None,
        };

        let state = Rc::new(RefCell::new(state));
        self.application_state
            .insert(remote, MigratedApplicationState::Registered(state.clone()));
        Some(state)
    }
}

/*************************************************************/
/* LOGGING QUEUE LENGTH */
/*************************************************************/

static mut LOG: Option<Vec<usize>> = None;
const GRANULARITY: i32 = 1; // Logs length after every GRANULARITY packets.

fn log_init() {
    unsafe {
        LOG = Some(Vec::with_capacity(1024 * 1024));
    }
}

fn log_len(len: usize) {
    static mut GRANULARITY_FLAG: i32 = GRANULARITY;

    unsafe {
        GRANULARITY_FLAG -= 1;
        if GRANULARITY_FLAG > 0 {
            return;
        }
        GRANULARITY_FLAG = GRANULARITY;
    }

    unsafe { LOG.as_mut().unwrap_unchecked() }.push(len);
}

pub fn log_print() {
    unsafe { LOG.as_ref().unwrap_unchecked() }
        .iter()
        .for_each(|len| println!("{}", len));
}

pub trait UserConnectionPeer {
    fn migrate_in(remote: SocketAddrV4, buffer: Buffer);

    fn migration_complete(remote: SocketAddrV4, qd: QDesc);

    type MigrateOut: UserConnectionMigrateOut;
    fn migrate_out(qd: QDesc) -> Self::MigrateOut;
}

pub trait UserConnectionMigrateOut {
    fn serialized_size(&self) -> usize;

    fn serialize<'buf>(&self, buf: &'buf mut [u8]) -> &'buf mut [u8];
}

pub mod user_connection {
    use std::{
        cell::Cell,
        collections::{
            hash_map::Entry,
            HashMap,
        },
        ffi::c_void,
        io::Write,
        net::SocketAddrV4,
        ptr::null,
    };

    use crate::{
        runtime::memory::Buffer,
        QDesc,
    };

    use super::UserConnectionMigrateOut;

    pub enum Peer {
        Nop,
        Buf(BufPeer),
        Ffi(FfiPeer),
        Erased(ErasedPeer),
    }

    pub enum MigrateOut {
        Nop,
        Buf(Vec<u8>),
        Ffi(*const c_void),
        Erased(ErasedMigrateOut),
    }

    impl Peer {
        pub fn migrate_in(&mut self, remote: SocketAddrV4, data: Buffer) {
            match self {
                Self::Nop => {},
                Self::Buf(peer) => peer.migrate_in(remote, data),
                Self::Ffi(peer) => peer.migrate_in(remote, data),
                Self::Erased(peer) => (peer.migrate_in)(remote, data),
            }
        }

        pub fn migration_complete(&mut self, remote: SocketAddrV4, qd: QDesc) {
            match self {
                Self::Nop => {},
                Self::Buf(peer) => peer.migration_complete(remote, qd),
                Self::Ffi(peer) => peer.migration_complete(remote, qd),
                Self::Erased(peer) => (peer.migration_complete)(remote, qd),
            }
        }

        pub fn migrate_out(&mut self, qd: QDesc) -> MigrateOut {
            match self {
                Self::Nop => MigrateOut::Nop,
                Self::Buf(peer) => peer.migrate_out(qd),
                Self::Ffi(peer) => peer.migrate_out(qd),
                Self::Erased(peer) => (peer.migrate_out)(qd),
            }
        }
    }

    impl MigrateOut {
        pub fn serialized_size(&self) -> usize {
            match self {
                Self::Nop => 0,
                Self::Buf(data) => data.len(),
                Self::Ffi(data) => unsafe { (FFI.get().serialized_size)(*data) },
                Self::Erased(data) => data.serialized_size,
            }
        }

        pub fn serialize<'buf>(&self, mut buf: &'buf mut [u8]) -> &'buf mut [u8] {
            match self {
                Self::Nop => buf,
                Self::Buf(data) => {
                    (&mut buf).write_all(data).expect("can write");
                    buf
                },
                Self::Ffi(data) => {
                    let remaining_len = unsafe { (FFI.get().serialize)(*data, buf.as_mut_ptr(), buf.len()) };
                    let used_len = buf.len() - remaining_len;
                    &mut buf[used_len..]
                },
                Self::Erased(data) => (data.serialize)(buf),
            }
        }
    }

    #[derive(Default)]
    pub struct BufPeer {
        connections: HashMap<QDesc, Vec<u8>>,
        migrating: HashMap<SocketAddrV4, Buffer>,
    }

    impl BufPeer {
        fn migrate_in(&mut self, remote: SocketAddrV4, data: Buffer) {
            let replaced = self.migrating.insert(remote, data);
            assert!(replaced.is_none())
        }

        fn migration_complete(&mut self, remote: SocketAddrV4, qd: QDesc) {
            let data = self.migrating.remove(&remote).expect("exist migrating data");
            let replaced = self.connections.insert(qd, data.to_vec());
            assert!(replaced.is_none())
        }

        fn migrate_out(&mut self, qd: QDesc) -> MigrateOut {
            MigrateOut::Buf(self.connections.remove(&qd).expect("exist connection data"))
        }

        pub fn entry(&mut self, qd: QDesc) -> Entry<'_, QDesc, Vec<u8>> {
            self.connections.entry(qd)
        }
    }

    #[derive(Clone, Copy)]
    pub struct Ffi {
        pub migrate_in: unsafe extern "C" fn(i32, *const u8, usize),
        pub migrate_out: unsafe extern "C" fn(i32) -> *const c_void,
        pub serialized_size: unsafe extern "C" fn(*const c_void) -> usize,
        pub serialize: unsafe extern "C" fn(*const c_void, *mut u8, usize) -> usize,
    }

    #[allow(unused)]
    unsafe extern "C" fn default_migrate_in(fd: i32, buf: *const u8, buf_len: usize) {}
    #[allow(unused)]
    unsafe extern "C" fn default_migrate_out(fd: i32) -> *const c_void {
        null()
    }
    #[allow(unused)]
    unsafe extern "C" fn default_serialized_size(data: *const c_void) -> usize {
        0
    }
    #[allow(unused)]
    unsafe extern "C" fn default_serialize(data: *const c_void, buf: *mut u8, buf_len: usize) -> usize {
        0
    }

    thread_local! {
        pub static FFI: Cell<Ffi> = Cell::new(Ffi {
            migrate_in: default_migrate_in,
            migrate_out: default_migrate_out,
            serialized_size: default_serialized_size,
            serialize: default_serialize
        })
    }

    #[derive(Default)]
    pub struct FfiPeer {
        migrating: HashMap<SocketAddrV4, Buffer>,
    }

    impl FfiPeer {
        fn migrate_in(&mut self, remote: SocketAddrV4, data: Buffer) {
            let replaced = self.migrating.insert(remote, data);
            assert!(replaced.is_none())
        }

        fn migration_complete(&mut self, remote: SocketAddrV4, qd: QDesc) {
            let data = self.migrating.remove(&remote).expect("exist migrating data");
            unsafe { (FFI.get().migrate_in)(qd.into(), data.as_ptr(), data.len()) }
        }

        fn migrate_out(&mut self, qd: QDesc) -> MigrateOut {
            MigrateOut::Ffi(unsafe { (FFI.get().migrate_out)(qd.into()) })
        }
    }

    pub struct ErasedPeer {
        migrate_in: fn(SocketAddrV4, Buffer),
        migration_complete: fn(SocketAddrV4, QDesc),
        migrate_out: fn(QDesc) -> MigrateOut,
    }

    pub struct ErasedMigrateOut {
        serialized_size: usize,
        serialize: Box<dyn Fn(&mut [u8]) -> &mut [u8]>,
    }

    impl ErasedPeer {
        pub fn new<T: super::UserConnectionPeer>() -> Self
        where
            T::MigrateOut: 'static,
        {
            Self {
                migrate_in: T::migrate_in,
                migration_complete: T::migration_complete,
                migrate_out: erased_migrate_out::<T>,
            }
        }
    }

    fn erased_migrate_out<T: super::UserConnectionPeer>(qd: QDesc) -> MigrateOut
    where
        T::MigrateOut: 'static,
    {
        let migrate_out = T::migrate_out(qd);
        MigrateOut::Erased(ErasedMigrateOut {
            serialized_size: migrate_out.serialized_size(),
            serialize: Box::new(move |buf| migrate_out.serialize(buf)),
        })
    }
}

pub fn user_connection_entry<T>(libos: &crate::LibOS, qd: QDesc, f: impl FnOnce(Entry<'_, QDesc, Vec<u8>>) -> T) -> T {
    let crate::LibOS::NetworkLibOS(crate::demikernel::libos::network::NetworkLibOS::Catnip(libos)) = libos else {
        unimplemented!()
    };
    libos.ipv4.tcp.with_mig_peer(|mig_peer| {
        let user_connection::Peer::Buf(peer) = &mut mig_peer.user_connection else {
            unimplemented!()
        };
        f(peer.entry(qd))
    })
}

pub fn set_user_connection_peer_buf(libos: &crate::LibOS) {
    let crate::LibOS::NetworkLibOS(crate::demikernel::libos::network::NetworkLibOS::Catnip(libos)) = libos else {
        unimplemented!()
    };
    libos
        .ipv4
        .tcp
        .with_mig_peer(|mig_peer| mig_peer.user_connection = user_connection::Peer::Buf(Default::default()))
}

pub unsafe fn set_user_connection_peer_ffi(
    libos: &crate::LibOS,
    migrate_in: unsafe extern "C" fn(i32, *const u8, usize),
    migrate_out: unsafe extern "C" fn(i32) -> *const std::ffi::c_void,
    serialized_size: unsafe extern "C" fn(*const std::ffi::c_void) -> usize,
    serialize: unsafe extern "C" fn(*const c_void, *mut u8, usize) -> usize,
) {
    user_connection::FFI.set(user_connection::Ffi {
        migrate_in,
        migrate_out,
        serialized_size,
        serialize,
    });
    let crate::LibOS::NetworkLibOS(crate::demikernel::libos::network::NetworkLibOS::Catnip(libos)) = libos else {
        unimplemented!()
    };
    libos
        .ipv4
        .tcp
        .with_mig_peer(|mig_peer| mig_peer.user_connection = user_connection::Peer::Ffi(Default::default()))
}

pub fn set_user_connection_peer<T: UserConnectionPeer>(libos: &crate::LibOS)
where
    T::MigrateOut: 'static,
{
    let crate::LibOS::NetworkLibOS(crate::demikernel::libos::network::NetworkLibOS::Catnip(libos)) = libos else {
        unimplemented!()
    };
    libos.ipv4.tcp.with_mig_peer(|mig_peer| {
        mig_peer.user_connection = user_connection::Peer::Erased(user_connection::ErasedPeer::new::<T>())
    })
}
