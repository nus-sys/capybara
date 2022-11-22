pub mod segment;
mod peer;
mod active;

pub use peer::TcpMigPeer;

//======================================================================================================================
// Structures
//======================================================================================================================

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationStage {
    None = 0,
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
            0 => Ok(None),
            1 => Ok(PrepareMigration),
            2 => Ok(PrepareMigrationAck),
            3 => Ok(ConnectionState),
            4 => Ok(ConnectionStateAck),
            e => Err(e),
        }
    }
}