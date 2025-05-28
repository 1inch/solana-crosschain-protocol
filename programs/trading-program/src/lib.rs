use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar::instructions::ID as IX_ID;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};
use borsh::BorshDeserialize;

pub mod constants;
pub mod utils;
use cross_chain_escrow_src::{
    cpi::{accounts::Create, create},
    program::CrossChainEscrowSrc,
};
use utils::{assert_pda, error::TradingProgramError, verify_order_signature};

declare_id!("5ahQ9NWeDmVKG3dJza1ZrFRoJ9wbEUM271HCvfHpAqFC");

#[program]
pub mod trading_program {

    use super::*;

    pub fn init_escrow_src(ctx: Context<InitEscrowSrc>) -> Result<()> {
        // 0 is the index of the instruction in the transaction
        let order = verify_order_signature(&ctx.accounts.ix_sysvar, 0)?;
        // Verify order data matches accounts
        if order.token != ctx.accounts.token.to_account_info().key() {
            return Err(TradingProgramError::OrderDataMismatch.into());
        }

        // Trading account address is validated here because
        let trading_account_bump = assert_pda(
            &ctx.accounts.trading_account,
            &[constants::SEED_PREFIX, order.maker.as_ref()],
        )?;

        // Initialize the escrow
        create(
            CpiContext::new_with_signer(
                ctx.accounts.escrow_src_program.to_account_info(),
                Create {
                    payer: ctx.accounts.taker.to_account_info(),
                    creator: ctx.accounts.trading_account.to_account_info(),
                    mint: ctx.accounts.token.to_account_info(),
                    creator_ata: ctx.accounts.trading_account_ata.to_account_info(),
                    escrow: ctx.accounts.escrow.to_account_info(),
                    escrow_ata: ctx.accounts.escrow_ata.to_account_info(),
                    associated_token_program: ctx
                        .accounts
                        .associated_token_program
                        .to_account_info(),
                    token_program: ctx.accounts.token_program.to_account_info(),
                    rent: ctx.accounts.rent.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                },
                &[&[
                    constants::SEED_PREFIX,
                    order.maker.as_ref(),
                    &[trading_account_bump],
                ]],
            ),
            order.order_hash,
            order.hashlock,
            order.amount,
            order.safety_deposit,
            ctx.accounts.taker.key(),
            order.finality_duration,
            order.withdrawal_duration,
            order.public_withdrawal_duration,
            order.cancellation_duration,
            order.rescue_start,
            order.dst_amount,
            order.dutch_auction_data.clone(),
        )?;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitEscrowSrc<'info> {
    #[account(mut)]
    pub taker: Signer<'info>,

    /// CHECK: check is not needed here as we never initialize the account
    pub trading_account: UncheckedAccount<'info>,

    #[account(
        mut,
        associated_token::mint = token,
        associated_token::authority = trading_account,
        associated_token::token_program = token_program
    )]
    pub trading_account_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: Verification done by CPI to escrow program
    #[account(mut)]
    pub escrow: UncheckedAccount<'info>,

    pub token: Box<InterfaceAccount<'info, Mint>>,

    /// CHECK: Verification done by CPI to escrow program
    #[account(mut)]
    pub escrow_ata: UncheckedAccount<'info>,

    /// CHECK: Address verification is done in constraint
    #[account(address = IX_ID)]
    pub ix_sysvar: UncheckedAccount<'info>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub rent: Sysvar<'info, Rent>,
    pub system_program: Program<'info, System>,
    pub escrow_src_program: Program<'info, CrossChainEscrowSrc>,
}
