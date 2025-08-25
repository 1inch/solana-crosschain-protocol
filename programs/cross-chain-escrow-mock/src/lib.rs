use anchor_lang::prelude::*;
use anchor_spl::token_interface::Mint;
pub use common::constants;

declare_id!("B9SnVJbXNd6RFNxHqPkTvdr46YPT17xunemTQfDsCNzA");

#[program]
pub mod cross_chain_escrow_mock {

    use common::{error::EscrowError, utils};

    use super::*;

    pub fn create(
        _ctx: Context<Create>,
        _order_hash: [u8; 32],
        _hashlock: [u8; 32],
        _amount: u64,
        _safety_deposit: u64,
        _recipient: Pubkey,
        _finality_duration: u32,
        _withdrawal_duration: u32,
        _public_withdrawal_duration: u32,
        _src_cancellation_timestamp: u32,
        _rescue_start: u32,
        _asset_is_native: bool,
        _escrow_bump: u8,
    ) -> Result<()> {
        Ok(())
    }

    pub fn create_and_set_fields(
        ctx: Context<Create>,
        order_hash: [u8; 32],
        hashlock: [u8; 32],
        amount: u64,
        safety_deposit: u64,
        recipient: Pubkey,
        finality_duration: u32,
        withdrawal_duration: u32,
        public_withdrawal_duration: u32,
        src_cancellation_timestamp: u32,
        rescue_start: u32,
        asset_is_native: bool,
        _escrow_bump: u8,
    ) -> Result<()> {
        let now = utils::get_current_timestamp()?;

        let withdrawal_start = now
            .checked_add(finality_duration)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let public_withdrawal_start = withdrawal_start
            .checked_add(withdrawal_duration)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let cancellation_start = public_withdrawal_start
            .checked_add(public_withdrawal_duration)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        require!(
            cancellation_start <= src_cancellation_timestamp,
            EscrowError::InvalidCreationTime
        );

        ctx.accounts.escrow.set_inner(EscrowDst {
            order_hash,
            hashlock,
            creator: ctx.accounts.creator.key(),
            recipient,
            token: ctx.accounts.mint.key(),
            amount,
            safety_deposit,
            withdrawal_start,
            public_withdrawal_start,
            cancellation_start,
            rescue_start,
            asset_is_native,
        });

        Ok(())
    }

    pub fn create_no_pda(
        _ctx: Context<CreateNoPda>,
        _order_hash: [u8; 32],
        _hashlock: [u8; 32],
        _amount: u64,
        _safety_deposit: u64,
        _recipient: Pubkey,
        _finality_duration: u32,
        _withdrawal_duration: u32,
        _public_withdrawal_duration: u32,
        _src_cancellation_timestamp: u32,
        _rescue_start: u32,
        _asset_is_native: bool,
        _escrow_bump: u8,
    ) -> Result<()> {
        Ok(())
    }

    pub fn create_no_pda_with_timestamp_checks(
        _ctx: Context<CreateNoPda>,
        _order_hash: [u8; 32],
        _hashlock: [u8; 32],
        _amount: u64,
        _safety_deposit: u64,
        _recipient: Pubkey,
        finality_duration: u32,
        withdrawal_duration: u32,
        public_withdrawal_duration: u32,
        src_cancellation_timestamp: u32,
        _rescue_start: u32,
        _asset_is_native: bool,
        _escrow_bump: u8,
    ) -> Result<()> {
        let now = utils::get_current_timestamp()?;

        let withdrawal_start = now
            .checked_add(finality_duration)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let public_withdrawal_start = withdrawal_start
            .checked_add(withdrawal_duration)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let cancellation_start = public_withdrawal_start
            .checked_add(public_withdrawal_duration)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        require!(
            cancellation_start <= src_cancellation_timestamp,
            EscrowError::InvalidCreationTime
        );

        Ok(())
    }

    #[derive(Accounts)]
    #[instruction(order_hash: [u8; 32], hashlock: [u8; 32], amount: u64, safety_deposit: u64, recipient: Pubkey, finality_duration: u32, withdrawal_duration: u32, public_withdrawal_duration: u32, src_cancellation_timestamp: u32, rescue_start: u32)]
    pub struct Create<'info> {
        #[account(
        mut, // Needed because this account transfers lamports if the token is native and to pay for the order creation
    )]
        creator: Signer<'info>,
        mint: Box<InterfaceAccount<'info, Mint>>,
        #[account(
            init,
            payer = creator,
            space = 8 + EscrowDst::INIT_SPACE,
            seeds = [ "escrow".as_bytes(),
            order_hash.as_ref(),
            hashlock.as_ref(),
            creator.key().as_ref(),
            recipient.as_ref(),
            mint.key().as_ref(),
            amount.to_be_bytes().as_ref(),
            safety_deposit.to_be_bytes().as_ref(),
            rescue_start.to_be_bytes().as_ref(),
            ],
        bump
    )]
        escrow: Account<'info, EscrowDst>,
        system_program: Program<'info, System>,
    }
}

#[derive(Accounts)]
#[instruction(order_hash: [u8; 32], hashlock: [u8; 32], amount: u64, safety_deposit: u64, recipient: Pubkey, finality_duration: u32, withdrawal_duration: u32, public_withdrawal_duration: u32, src_cancellation_timestamp: u32, rescue_start: u32, asset_is_native: bool, escrow_bump: u8)]
pub struct CreateNoPda<'info> {
    #[account(
        mut, // Needed because this account transfers lamports if the token is native and to pay for the order creation
    )]
    creator: Signer<'info>,
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
            seeds = [ "escrow".as_bytes(),
            order_hash.as_ref(),
            hashlock.as_ref(),
            creator.key().as_ref(),
            recipient.as_ref(),
            mint.key().as_ref(),
            amount.to_be_bytes().as_ref(),
            safety_deposit.to_be_bytes().as_ref(),
            rescue_start.to_be_bytes().as_ref(),
            ],
        bump = escrow_bump
    )]
    /// CHECK: safe
    escrow: AccountInfo<'info>,
    system_program: Program<'info, System>,
}

#[account]
#[derive(InitSpace)]
pub struct EscrowDst {
    order_hash: [u8; 32],
    hashlock: [u8; 32],
    creator: Pubkey,
    recipient: Pubkey,
    token: Pubkey,
    asset_is_native: bool,
    amount: u64,
    safety_deposit: u64,
    withdrawal_start: u32,
    public_withdrawal_start: u32,
    cancellation_start: u32,
    rescue_start: u32,
}
