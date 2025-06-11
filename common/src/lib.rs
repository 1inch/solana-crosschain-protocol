pub mod constants;
pub mod error;
pub mod escrow;
pub mod utils;

pub use primitive_types::U256;

use borsh::{BorshDeserialize, BorshSerialize};

impl BorshSerialize for U256 {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let mut buf = [0u8; 32];
        self.to_little_endian(&mut buf);
        writer.write_all(&buf)
    }
}

impl BorshDeserialize for U256 {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let mut buf = [0u8; 32];
        reader.read_exact(&mut buf)?;
        Ok(U256::from_little_endian(&buf))
    }
}
