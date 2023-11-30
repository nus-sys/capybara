

//=============================================
//  Structures
//=============================================

/* header prism_req_base_h {
    0 bit<8>  type; // 0: add, 1: delete, 2: chown, 3: lock, 4: unlock
    1..3 bit<16>  status;

    3..7 bit<32>  peer_addr; // peer: client
    7..9 bit<16>  peer_port;
}

header prism_add_req_h {
    9..13 bit<32>  virtual_addr; // virtual: frontend
    13..15 bit<16>  virtual_port;
    15..19 bit<32>  owner_addr; // owner: frontend
    19..21 bit<16>  owner_port;
    21..27 bit<48> owner_mac;

    27 bit<8> lock;
}

header prism_chown_req_h {
    9..13 bit<32>  owner_addr; // owner: backend http server
    13..15 bit<16>  owner_port;
    15..21 bit<48> owner_mac;

    21 bit<8> unlock;
} 

ADD: base + add (FE sends before migrating a connection)
DELETE: base (BE sends once it receives FIN)
CHOWN: base + chown (BE sends after it receives a migrated connection)
LOCK: base (BE sends once it receives the next request)

*/

use std::net::{SocketAddrV4, Ipv4Addr};

use demikernel::{runtime::memory::{Buffer, DataBuffer}, MacAddress};
use num_traits::ToBytes;

#[derive(Debug)]
pub struct PrismPacket {
    pub client: SocketAddrV4,
    pub kind: PrismType,
}

#[derive(Debug)]
pub enum PrismType {
    Add(SocketAddrV4, MacAddress), // frontend addr
    Chown(SocketAddrV4, MacAddress), // backend addr
    Lock,
}

//=============================================
//  Implementations
//=============================================

impl PrismPacket {
    pub fn add(client: SocketAddrV4, fe: SocketAddrV4, fe_mac: MacAddress) -> Self {
        Self { client, kind: PrismType::Add(fe, fe_mac) }
    }

    pub fn chown(client: SocketAddrV4, be: SocketAddrV4, be_mac: MacAddress) -> Self {
        Self { client, kind: PrismType::Chown(be, be_mac) }
    }

    pub fn lock(client: SocketAddrV4) -> Self {
        Self { client, kind: PrismType::Lock }
    }

    pub fn serialize<'a>(&self, buf: &'a mut [u8]) -> &'a [u8] {
        let size: usize = 9 + match self.kind {
            PrismType::Add(..) => 19, // 28
            PrismType::Chown(..) => 13, // 22
            PrismType::Lock => 0, // 9
        };

        let buf = &mut buf[..size];

        buf[0] = self.kind.encode();
        buf[1] = 0;
        buf[2] = 0;
        buf[3..9].encode(self.client);

        match self.kind {
            PrismType::Add(fe, fe_mac) => {
                buf[9..15].encode(fe);
                buf[15..21].encode(fe);
                buf[21..27].copy_from_slice(fe_mac.as_bytes());
                buf[27] = 1;
            },

            PrismType::Chown(be, be_mac) => {
                buf[9..15].encode(be);
                buf[15..21].copy_from_slice(be_mac.as_bytes());
                buf[21] = 1;
            },

            PrismType::Lock => (),
        }

        buf
    }

    pub fn deserialize(buf: &[u8]) -> Result<Self, String> {
        let client = buf[3..9].decode();
        match buf[0] {
            // ADD
            0 => {
                let fe = buf[15..21].decode();
                let fe_mac = MacAddress::from_bytes(&buf[21..27]);
                Ok(Self::add(client, fe, fe_mac))
            },

            // CHOWN
            2 => {
                let be = buf[9..15].decode();
                let be_mac = MacAddress::from_bytes(&buf[15..21]);
                Ok(Self::chown(client, be, be_mac))
            },

            // LOCK
            3 => {
                Ok(Self::lock(client))
            },

            e => Err(format!("invalid type: {e}")),
        }
    }
}

impl PrismType {
    fn encode(&self) -> u8 {
        match self {
            PrismType::Add(..) => 0,
            PrismType::Chown(..) => 2,
            PrismType::Lock => 3,
        }
    }
}

trait EncodeAddr {
    fn encode(&mut self, addr: SocketAddrV4);
    fn decode(&self) -> SocketAddrV4;
}

impl EncodeAddr for [u8] {
    fn encode(&mut self, addr: SocketAddrV4) {
        self[0..4].copy_from_slice(&addr.ip().octets());
        self[4..6].copy_from_slice(&addr.port().to_be_bytes());
    }

    fn decode(&self) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(self[0], self[1], self[2], self[3]), u16::from_be_bytes(self[4..6].try_into().unwrap()))
    }
}