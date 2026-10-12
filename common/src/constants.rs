/// Discriminator size in bytes
pub const DISCRIMINATOR_BYTES: usize = 8;

/// The time between the creation of the escrow and the start of the ability to rescue tokens, equal to 8 days.
pub const RESCUE_DELAY: u32 = 691200;

pub const BASE_1E7: u64 = 10_000_000;

/// Base for fee percentages (`protocol_fee`, `integrator_fee`), 100% = `BASE_1E5`.
/// Matches `_BASE_1E5` of the EVM `AmountGetterWithFee`.
pub const BASE_1E5: u64 = 100_000;

/// Base for the integrator share, 100% = `BASE_1E2`.
/// Matches `_BASE_1E2` of the EVM `AmountGetterWithFee`.
pub const BASE_1E2: u64 = 100;
