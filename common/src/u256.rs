pub use primitive_types::U256 as PrimitiveU256;
use borsh::{BorshDeserialize, BorshSerialize};
use core::ops::{Add, Div, Mul, Sub};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
#[repr(transparent)]
pub struct U256(pub PrimitiveU256);

impl U256 {
    pub const fn from(value: u64) -> Self {
        Self(PrimitiveU256([value, 0, 0, 0]))
    }

    pub fn from_little_endian(slice: &[u8]) -> Self {
        Self(PrimitiveU256::from_little_endian(slice))
    }

    pub fn to_little_endian(&self, slice: &mut [u8]) {
        self.0.to_little_endian(slice);
    }

    pub const fn zero() -> Self {
        Self(PrimitiveU256([0, 0, 0, 0]))
    }

    pub const fn one() -> Self {
        Self(PrimitiveU256([1, 0, 0, 0]))
    }

    pub fn checked_mul(self, rhs: Self) -> Option<Self> {
        self.0.checked_mul(rhs.0).map(U256)
    }

    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        self.0.checked_add(rhs.0).map(U256)
    }
}

impl From<u64> for U256 {
    fn from(value: u64) -> Self {
        Self(PrimitiveU256::from(value))
    }
}

impl From<PrimitiveU256> for U256 {
    fn from(value: PrimitiveU256) -> Self {
        Self(value)
    }
}

impl From<U256> for PrimitiveU256 {
    fn from(value: U256) -> Self {
        value.0
    }
}

impl BorshSerialize for U256 {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let mut buf = [0u8; 32];
        self.0.to_little_endian(&mut buf);
        writer.write_all(&buf)
    }
}

impl BorshDeserialize for U256 {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let mut buf = [0u8; 32];
        reader.read_exact(&mut buf)?;
        Ok(Self(PrimitiveU256::from_little_endian(&buf)))
    }
}

impl Add for U256 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        U256(self.0 + rhs.0)
    }
}

impl Sub for U256 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        U256(self.0 - rhs.0)
    }
}

impl Mul for U256 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        U256(self.0 * rhs.0)
    }
}

impl Div for U256 {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        U256(self.0 / rhs.0)
    }
}
