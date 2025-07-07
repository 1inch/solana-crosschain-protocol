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
    #[msg("Invalid amount")]
    InvalidAmount,
    #[msg("Invalid parts amount")]
    InvalidPartsAmount,
    #[msg("Invalid creation time")]
    InvalidCreationTime,
    #[msg("Invalid time")]
    InvalidTime,
    #[msg("Invalid rescue start")]
    InvalidRescueStart,
    #[msg("Invalid mint")]
    InvalidMint,
    #[msg("Missing creator ata")]
    MissingCreatorAta,
    #[msg("Missing recipient ata")]
    MissingRecipientAta,
    #[msg("Inconsistent native trait")]
    InconsistentNativeTrait,
    #[msg("Cancel by resolver is forbidden")]
    CancelOrderByResolverIsForbidden,
    #[msg("Order not expired")]
    OrderNotExpired,
    #[msg("Order has expired")]
    OrderHasExpired,
    #[msg("Dutch auction data hash mismatch")]
    DutchAuctionDataHashMismatch,
    #[msg("Invalid cancellation fee")]
    InvalidCancellationFee,
    #[msg("Invalid merkle proof")]
    InvalidMerkleProof,
    #[msg("Invalid partial fill")]
    InvalidPartialFill,
    #[msg("Inconsistent merkle proof trait")]
    InconsistentMerkleProofTrait,
}
