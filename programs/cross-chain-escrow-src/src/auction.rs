use anchor_lang::prelude::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct PointAndTimeDelta {
    pub rate_bump: U24,
    pub time_delta: u16,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct AuctionData {
    pub start_time: u32,
    pub duration: u32,
    pub initial_rate_bump: U24,
    pub points_and_time_deltas: Vec<PointAndTimeDelta>,
}

pub fn calculate_rate_bump(timestamp: u64, data: &AuctionData) -> u64 {
    if timestamp <= data.start_time as u64 {
        return data.initial_rate_bump.to_u64();
    }
    let auction_finish_time = data.start_time as u64 + data.duration as u64;
    if timestamp >= auction_finish_time {
        return 0;
    }

    let mut current_rate_bump = data.initial_rate_bump.to_u64();
    let mut current_point_time = data.start_time as u64;

    for point_and_time_delta in data.points_and_time_deltas.iter() {
        let next_rate_bump = point_and_time_delta.rate_bump.to_u64();
        let point_time_delta = point_and_time_delta.time_delta as u64;
        let next_point_time = current_point_time + point_time_delta;

        if timestamp <= next_point_time {
            // Overflow is not possible because:
            // 1. current_point_time < timestamp <= next_point_time
            // 2. timestamp * rate_bump < 2^64
            // 3. point_time_delta != 0 as this would contradict point 1
            return ((timestamp - current_point_time) * next_rate_bump
                + (next_point_time - timestamp) * current_rate_bump)
                / point_time_delta;
        }

        current_rate_bump = next_rate_bump;
        current_point_time = next_point_time;
    }

    // Overflow is not possible because:
    // 1. timestamp < auction_finish_time
    // 2. rate_bump * timestamp < 2^64
    // 3. current_point_time < auction_finish_time as we know that current_point_time < timestamp
    current_rate_bump * (auction_finish_time - timestamp)
        / (auction_finish_time - current_point_time)
}

pub fn calculate_premium(
    timestamp: u32,
    auction_start_time: u32,
    auction_duration: u32,
    max_cancellation_premium: u64,
) -> u64 {
    if timestamp <= auction_start_time {
        return 0;
    }

    let time_elapsed = timestamp - auction_start_time;
    if time_elapsed >= auction_duration {
        return max_cancellation_premium;
    }

    (time_elapsed as u64 * max_cancellation_premium) / auction_duration as u64
}

#[derive(Clone, Copy, Default, AnchorSerialize, AnchorDeserialize)]
pub struct U24([u8; 3]);

impl U24 {
    pub fn to_u32(&self) -> u32 {
        ((self.0[0] as u32) << 16) | ((self.0[1] as u32) << 8) | (self.0[2] as u32)
    }

    pub fn to_u64(&self) -> u64 {
        self.to_u32() as u64
    }
}

impl From<u32> for U24 {
    fn from(val: u32) -> Self {
        assert!(val <= 0xFFFFFF, "U24 overflow");
        Self([(val >> 16) as u8, (val >> 8) as u8, val as u8])
    }
}

impl From<U24> for u32 {
    fn from(val: U24) -> u32 {
        val.to_u32()
    }
}

impl From<U24> for u64 {
    fn from(val: U24) -> u64 {
        val.to_u64()
    }
}
