use anchor_lang::prelude::*;
use anchor_spl::associated_token::{AssociatedToken, ID as ASSOCIATED_TOKEN_PROGRAM_ID};
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};
pub use common::constants;
use common::error::EscrowError;
use common::escrow::{EscrowBase, EscrowType};
use common::utils;

declare_id!("B9SnVJbXNd6RFNxHqPkTvdr46YPT17xunemTQfDsCNzA");

#[program]
pub mod cross_chain_escrow_mock {

    use super::*;

    pub fn create(
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
        escrow_bump: u8,
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

        // common::escrow::create(
        //     500 + constants::DISCRIMINATOR_BYTES,
        //     EscrowType::Dst,
        //     &ctx.accounts.creator,
        //     asset_is_native,
        //     &ctx.accounts.escrow_ata,
        //     ctx.accounts.creator_ata.as_deref(),
        //     &ctx.accounts.mint,
        //     &ctx.accounts.token_program,
        //     &ctx.accounts.system_program,
        //     amount,
        //     safety_deposit,
        //     rescue_start,
        //     now,
        // )?;

        Ok(())
    }

    #[derive(Accounts)]
    #[instruction(order_hash: [u8; 32], hashlock: [u8; 32], amount: u64, safety_deposit: u64, recipient: Pubkey, finality_duration: u32, withdrawal_duration: u32, public_withdrawal_duration: u32, src_cancellation_timestamp: u32, rescue_start: u32)]
    pub struct Create<'info> {
        /// Puts tokens into escrow
        #[account(
        mut, // Needed because this account transfers lamports if the token is native and to pay for the order creation
    )]
        creator: Signer<'info>,
        /// CHECK: check is not necessary as token is only used as a constraint to creator_ata and escrow_ata
        mint: Box<InterfaceAccount<'info, Mint>>,
        #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = creator,
        associated_token::token_program = token_program
    )]
        /// Account to store creator's tokens (Optional if the token is native)
        creator_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
        /// Account to store escrow details
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
        // CHECK: LMAO
        escrow: Account<'info, EscrowDst>,
        /// Account to store escrowed tokens
        //     #[account(
        //     init,
        //     payer = creator,
        //     associated_token::mint = mint,
        //     associated_token::authority = escrow,
        //     associated_token::token_program = token_program
        // )]
        //     escrow_ata: Box<InterfaceAccount<'info, TokenAccount>>,

        #[account(address = ASSOCIATED_TOKEN_PROGRAM_ID)]
        associated_token_program: Program<'info, AssociatedToken>,
        token_program: Interface<'info, TokenInterface>,
        rent: Sysvar<'info, Rent>,
        system_program: Program<'info, System>,
    }
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
