use anchor_lang::error_code;

#[error_code]
pub enum EscrowError {
    #[msg("Zero amount or deposit")]
    ZeroAmountOrDeposit,
    #[msg("Safety deposit too large")]
    SafetyDepositTooLarge,
    #[msg("Invalid secret")]
    InvalidSecret,
    #[msg("Invalid account")]
    InvalidAccount,
    #[msg("Invalid creation time")]
    InvalidCreationTime,
    #[msg("Invalid time")]
    InvalidTime,
    #[msg("Invalid rescue start")]
    InvalidRescueStart,
    #[msg("Missing creator ata")]
    MissingCreatorAta,
    #[msg("Missing recipient ata")]
    MissingRecipientAta,
    #[msg("Inconsistent native trait")]
    InconsistentNativeTrait,
    #[msg("Order has expired")]
    OrderHasExpired,
    #[msg("Dutch auction data hash mismatch")]
    DutchAuctionDataHashMismatch,
}
