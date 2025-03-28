use anchor_lang::prelude::*;

pub fn get_current_timestamp() -> Result<u32> {
    // 'unix_timestamp' has type i64, but the timestamp values
    // in accounts are stored as u32 to save space.
    // This cast is safe since the max u32 timestamp value
    // will be reached at 2106.
    Ok(Clock::get()?.unix_timestamp as u32)
}
