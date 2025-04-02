use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar::instructions::ID as IX_ID;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};
use borsh::BorshDeserialize;

use common::constants;
mod utils;
pub use utils::{error::TradingProgramError, verify_order_signature};
use cross_chain_escrow_dst::{cpi::{create as create_dst, accounts::Create as CreateDst}, program::CrossChainEscrowDst};
use cross_chain_escrow_src::{cpi::{create as create_src, accounts::Create as CreateSrc}, program::CrossChainEscrowSrc};

declare_id!("5ahQ9NWeDmVKG3dJza1ZrFRoJ9wbEUM271HCvfHpAqFC");

#[program]
pub mod trading_program {
    use super::*;

    pub fn create_trading_account(ctx: Context<CreateTradingAccount>) -> Result<()> {
        ctx.accounts.trading_account.owner = ctx.accounts.owner.key();
        Ok(())
    }

    pub fn init_escrow_dst(ctx: Context<InitEscrowDst>, src_cancellation_timestamp: u32, rescue_start: u32) -> Result<()> {
        // 0 is the index of the instruction in the transaction
        let (order_signer, order) = verify_order_signature(&ctx.accounts.ix_sysvar, 0)?;
        // Verify order data matches accounts
        if order_signer != ctx.accounts.trading_account.owner.key()
            || order.token != ctx.accounts.token.to_account_info().key()
        {
            return Err(TradingProgramError::OrderDataMismatch.into());
        }

        // Initialize the escrow
        create_dst(
            CpiContext::new_with_signer(
                ctx.accounts.escrow_dst_program.to_account_info(),
                CreateDst {
                    payer: ctx.accounts.taker.to_account_info(),
                    creator: ctx.accounts.trading_account.to_account_info(),
                    token: ctx.accounts.token.to_account_info(),
                    creator_ata: ctx.accounts.trading_account_tokens.to_account_info(),
                    escrow: ctx.accounts.escrow.to_account_info(),
                    escrow_ata: ctx.accounts.escrow_tokens.to_account_info(),
                    associated_token_program: ctx.accounts.associated_token_program.to_account_info(),
                    token_program: ctx.accounts.token_program.to_account_info(),
                    rent: ctx.accounts.rent.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                },
                &[&[
                    ctx.accounts.maker.key().as_ref(),
                    &[ctx.bumps.trading_account],
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
            src_cancellation_timestamp,
            rescue_start,
        )?;

        Ok(())
    }

    pub fn init_escrow_src(ctx: Context<InitEscrowSrc>, src_cancellation_timestamp: u32, rescue_start: u32) -> Result<()> {
        // 0 is the index of the instruction in the transaction
        let (order_signer, order) = verify_order_signature(&ctx.accounts.ix_sysvar, 0)?;
        // Verify order data matches accounts
        if order_signer != ctx.accounts.trading_account.owner.key()
            || order.token != ctx.accounts.token.to_account_info().key()
        {
            return Err(TradingProgramError::OrderDataMismatch.into());
        }

        // Initialize the escrow
        create_src(
            CpiContext::new_with_signer(
                ctx.accounts.escrow_src_program.to_account_info(),
                CreateSrc {
                    payer: ctx.accounts.taker.to_account_info(),
                    creator: ctx.accounts.trading_account.to_account_info(),
                    token: ctx.accounts.token.to_account_info(),
                    creator_ata: ctx.accounts.trading_account_tokens.to_account_info(),
                    escrow: ctx.accounts.escrow.to_account_info(),
                    escrow_ata: ctx.accounts.escrow_tokens.to_account_info(),
                    associated_token_program: ctx.accounts.associated_token_program.to_account_info(),
                    token_program: ctx.accounts.token_program.to_account_info(),
                    rent: ctx.accounts.rent.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                },
                &[&[
                    ctx.accounts.maker.key().as_ref(),
                    &[ctx.bumps.trading_account],
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
            src_cancellation_timestamp,
            rescue_start,
        )?;

        Ok(())
    }
}

#[account]
#[derive(InitSpace)]
pub struct TradingAccount {
    pub owner: Pubkey,
}

#[derive(Accounts)]
pub struct CreateTradingAccount<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        init,
        payer = owner,
        space = constants::DISCRIMINATOR + TradingAccount::INIT_SPACE,
        seeds = [owner.key.as_ref()],
        bump
    )]
    pub trading_account: Account<'info, TradingAccount>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitEscrowDst<'info> {
    #[account(mut)]
    pub taker: Signer<'info>,

    /// CHECK: actual maker address is needed to only derive the trading account address
    pub maker: AccountInfo<'info>,

    #[account(
        mut,
        seeds = [maker.key().as_ref()],
        bump
    )]
    pub trading_account: Account<'info, TradingAccount>,

    #[account(
        mut,
        associated_token::mint = token,
        associated_token::authority = trading_account,
    )]
    pub trading_account_tokens: Account<'info, TokenAccount>,

    /// CHECK: Verification done by CPI to escrow program
    #[account(mut)]
    pub escrow: AccountInfo<'info>,

    pub token: Account<'info, Mint>,

    /// CHECK: Verification done by CPI to escrow program
    #[account(mut)]
    pub escrow_tokens: AccountInfo<'info>,

    #[account(address = IX_ID)]
    /// CHECK: Address verification is done in constraint
    pub ix_sysvar: AccountInfo<'info>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
    pub system_program: Program<'info, System>,
    pub escrow_dst_program: Program<'info, CrossChainEscrowDst>,
}

#[derive(Accounts)]
pub struct InitEscrowSrc<'info> {
    #[account(mut)]
    pub taker: Signer<'info>,

    /// CHECK: actual maker address is needed to only derive the trading account address
    pub maker: AccountInfo<'info>,

    #[account(
        mut,
        seeds = [maker.key().as_ref()],
        bump
    )]
    pub trading_account: Account<'info, TradingAccount>,

    #[account(
        mut,
        associated_token::mint = token,
        associated_token::authority = trading_account,
    )]
    pub trading_account_tokens: Account<'info, TokenAccount>,

    /// CHECK: Verification done by CPI to escrow program
    #[account(mut)]
    pub escrow: AccountInfo<'info>,

    pub token: Account<'info, Mint>,

    /// CHECK: Verification done by CPI to escrow program
    #[account(mut)]
    pub escrow_tokens: AccountInfo<'info>,

    #[account(address = IX_ID)]
    /// CHECK: Address verification is done in constraint
    pub ix_sysvar: AccountInfo<'info>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
    pub system_program: Program<'info, System>,
    pub escrow_src_program: Program<'info, CrossChainEscrowSrc>,
}
