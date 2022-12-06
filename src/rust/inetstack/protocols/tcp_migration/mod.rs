pub mod segment;
mod peer;
mod active;
pub mod stats;

pub use peer::TcpMigPeer;

//======================================================================================================================
// Structures
//======================================================================================================================

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationStage {
    Rejected = 0,
    None,
    PrepareMigration,
    PrepareMigrationAck,
    ConnectionState,
    ConnectionStateAck,
}

//======================================================================================================================
// Standard Library Trait Implementations
//======================================================================================================================

impl From<MigrationStage> for u8 {
    fn from(value: MigrationStage) -> Self {
        value as u8
    }
}

impl TryFrom<u8> for MigrationStage {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, u8> {
        use MigrationStage::*;
        match value {
            0 => Ok(Rejected),
            1 => Ok(None),
            2 => Ok(PrepareMigration),
            3 => Ok(PrepareMigrationAck),
            4 => Ok(ConnectionState),
            5 => Ok(ConnectionStateAck),
            e => Err(e),
        }
    }
}