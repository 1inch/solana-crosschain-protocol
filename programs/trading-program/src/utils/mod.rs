use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::Instruction, sysvar::instructions::load_instruction_at_checked,
};
use borsh::{BorshDeserialize, BorshSerialize};

/// This mod contains functions that validate that an instruction
/// is constructed the way we expect. In this case, this is for
/// `Ed25519Program.createInstructionWithPublicKey()` and
/// `Secp256k1Program.createInstructionWithEthAddress()` instructions.
pub mod ed25519;
pub mod error;

pub use ed25519::resolve_order;

#[derive(BorshSerialize, BorshDeserialize)]
pub struct Order {
    pub order_hash: [u8; 32],
    pub hashlock: [u8; 32],
    pub maker: Pubkey,
    pub token: Pubkey,
    pub amount: u64,
    pub safety_deposit: u64,
    pub finality_duration: u32,
    pub withdrawal_duration: u32,
    pub public_withdrawal_duration: u32,
    pub cancellation_duration: u32,
    pub rescue_start: u32,
    pub dst_chain_id: [u8; 32],
    pub dst_token: [u8; 32],
    pub dutch_auction_data: cross_chain_escrow_src::AuctionData,
}

/// Verifies that the order is signed by the maker
pub fn verify_order_signature(ix_sysvar: &AccountInfo, instruction_index: u8) -> Result<Order> {
    // Load instruction
    let ix: Instruction = load_instruction_at_checked(instruction_index.into(), ix_sysvar)?;

    // Resolve the order signer and data
    let (order_signer, order_data_raw) = resolve_order(&ix)?;

    // Deserialize the order
    let order = Order::try_from_slice(&order_data_raw)?;

    // Verify the signer matches the maker
    if order_signer != order.maker {
        return Err(error::TradingProgramError::SigVerificationFailed.into());
    }

    Ok(order)
}

pub fn assert_pda(account_info: &AccountInfo, seeds: &[&[u8]]) -> Result<u8> {
    let (pda, bump) =
        Pubkey::try_find_program_address(seeds, &crate::id()).ok_or(ErrorCode::ConstraintSeeds)?;
    require!(*account_info.key == pda, ErrorCode::ConstraintSeeds);
    Ok(bump)
}
