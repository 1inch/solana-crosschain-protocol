use anchor_lang::prelude::*;
use primitive_types::U256;

#[derive(Clone, Copy)]
pub struct Timelocks(pub U256);

#[repr(u8)]
pub enum Stage {
    SrcWithdrawal = 0,
    SrcPublicWithdrawal = 1,
    SrcCancellation = 2,
    SrcPublicCancellation = 3,
    DstWithdrawal = 4,
    DstPublicWithdrawal = 5,
    DstCancellation = 6,
}

const DEPLOYED_AT_OFFSET: usize = 224;
const STAGE_BIT_SIZE: usize = 32;
const DEPLOYED_AT_MASK: U256 = U256([0, 0, 0, 0xffffffff00000000]);

impl Timelocks {
    pub fn set_deployed_at(self, value: u32) -> Self {
        let cleared = self.0 & !DEPLOYED_AT_MASK;
        Self(cleared | (U256::from(value) << DEPLOYED_AT_OFFSET))
    }

    pub fn get(self, stage: Stage) -> std::result::Result<u32, ProgramError> {
        let shift = (stage as usize) * STAGE_BIT_SIZE;
        let deployed_at = (self.0 >> DEPLOYED_AT_OFFSET).as_u32();
        let delta = ((self.0 >> shift) & U256::from(u32::MAX)).as_u32();
        let result = deployed_at
            .checked_add(delta)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        Ok(result)
    }
}
