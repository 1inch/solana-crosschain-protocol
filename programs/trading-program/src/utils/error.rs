use anchor_lang::prelude::*;

/// Custom error codes
#[error_code]
pub enum TradingProgramError {
    #[msg("Signature verification failed.")]
    SigVerificationFailed,

    #[msg("Order data mismatch.")]
    OrderDataMismatch,
}
