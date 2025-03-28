use anchor_lang::error_code;

#[error_code]
pub enum EscrowError {
    #[msg("ZeroAmountOrDeposit")]
    ZeroAmountOrDeposit,
    #[msg("SafetyDepositTooLarge")]
    SafetyDepositTooLarge,
    #[msg("Invalid secret")]
    InvalidSecret,
    #[msg("Invalid account")]
    InvalidAccount,
    #[msg("Invalid creation time")]
    InvalidCreationTime,
    #[msg("Invalid time")]
    InvalidTime,
    #[msg("Invalid rescue delay")]
    InvalidRescueDelay,
}
