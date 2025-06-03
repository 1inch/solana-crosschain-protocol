use anchor_lang::prelude::*;
use anchor_lang::solana_program::hash::hashv;
use anchor_spl::associated_token::{AssociatedToken, ID as ASSOCIATED_TOKEN_PROGRAM_ID};
use anchor_spl::token_interface::{
    close_account, CloseAccount, Mint, TokenAccount, TokenInterface,
};
pub use auction::{calculate_rate_bump, AuctionData};
pub use common::constants;
use common::error::EscrowError;
use common::escrow::{uni_transfer, EscrowBase, UniTransferParams};
use common::utils;
use muldiv::MulDiv;

pub mod auction;

declare_id!("6NwMYeUmigiMDjhYeYpbxC6Kc63NzZy1dfGd7fGcdkVS");

#[program]
pub mod cross_chain_escrow_src {
    use super::*;

    pub fn create(
        ctx: Context<Create>,
        order_hash: [u8; 32],
        hashlock: [u8; 32],
        amount: u64,
        safety_deposit: u64,
        finality_duration: u32,
        withdrawal_duration: u32,
        public_withdrawal_duration: u32,
        cancellation_duration: u32,
        rescue_start: u32,
        expiration_duration: u32,
        asset_is_native: bool,
        dst_amount: u64,
        dutch_auction_data_hash: [u8; 32],
    ) -> Result<()> {
        let now = utils::get_current_timestamp()?;

        require!(expiration_duration != 0, EscrowError::InvalidTime);

        let expiration_time = now
            .checked_add(expiration_duration)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        common::escrow::create(
            EscrowSrc::INIT_SPACE + constants::DISCRIMINATOR_BYTES, // Needed to check the safety deposit amount validity
            &ctx.accounts.creator,
            asset_is_native,
            &ctx.accounts.order_ata,
            ctx.accounts.creator_ata.as_deref(),
            &ctx.accounts.mint,
            &ctx.accounts.token_program,
            &ctx.accounts.system_program,
            amount,
            safety_deposit,
            rescue_start,
            now,
        )?;

        let order = &mut ctx.accounts.order;
        order.set_inner(Order {
            order_hash,
            hashlock,
            creator: ctx.accounts.creator.key(),
            token: ctx.accounts.mint.key(),
            amount,
            safety_deposit,
            finality_duration,
            withdrawal_duration,
            public_withdrawal_duration,
            cancellation_duration,
            rescue_start,
            expiration_time,
            asset_is_native,
            dst_amount,
            dutch_auction_data_hash,
        });

        Ok(())
    }

    pub fn create_escrow(
        ctx: Context<CreateEscrow>,
        dutch_auction_data: AuctionData,
    ) -> Result<()> {
        let order = &ctx.accounts.order;
        let escrow = &mut ctx.accounts.escrow;

        let now = utils::get_current_timestamp()?;

        require!(now < order.expiration_time, EscrowError::OrderHasExpired);
        let calculated_hash = hashv(&[&dutch_auction_data.try_to_vec()?]).to_bytes();
        require!(
            calculated_hash == order.dutch_auction_data_hash,
            EscrowError::DutchAuctionDataHashMismatch
        );

        let withdrawal_start = now
            .checked_add(order.finality_duration)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let public_withdrawal_start = withdrawal_start
            .checked_add(order.withdrawal_duration)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let cancellation_start = public_withdrawal_start
            .checked_add(order.public_withdrawal_duration)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let public_cancellation_start = cancellation_start
            .checked_add(order.cancellation_duration)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        let seeds = [
            "order".as_bytes(),
            &order.order_hash,
            &order.hashlock,
            order.creator.as_ref(),
            order.token.as_ref(),
            &order.amount.to_be_bytes(),
            &order.safety_deposit.to_be_bytes(),
            &order.rescue_start.to_be_bytes(),
            &[ctx.bumps.order],
        ];

        uni_transfer(
            &UniTransferParams::TokenTransfer {
                from: ctx.accounts.order_ata.to_account_info(),
                authority: order.to_account_info(),
                to: ctx.accounts.escrow_ata.to_account_info(),
                mint: *ctx.accounts.mint.clone(),
                amount: order.amount,
                program: ctx.accounts.token_program.clone(),
            },
            Some(&[&seeds]),
        )?;

        escrow.set_inner(EscrowSrc {
            order_hash: order.order_hash,
            hashlock: order.hashlock,
            maker: order.creator,
            taker: ctx.accounts.taker.key(),
            token: order.token,
            amount: order.amount,
            safety_deposit: order.safety_deposit,
            withdrawal_start,
            public_withdrawal_start,
            cancellation_start,
            public_cancellation_start,
            rescue_start: order.rescue_start,
            asset_is_native: order.asset_is_native,
            dst_amount: get_dst_amount(order.dst_amount, &dutch_auction_data)?,
        });

        // Close the order_ata account
        close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            CloseAccount {
                account: ctx.accounts.order_ata.to_account_info(),
                destination: ctx.accounts.maker.to_account_info(),
                authority: order.to_account_info(),
            },
            &[&seeds],
        ))?;

        // Close the order account
        order.close(ctx.accounts.maker.to_account_info())?;

        Ok(())
    }

    // TODO! Fix withdrawal and cancellation logic in SOL-120, SOL-121, SOL-122

    pub fn withdraw(ctx: Context<Withdraw>, secret: [u8; 32]) -> Result<()> {
        let now = utils::get_current_timestamp()?;
        require!(
            now >= ctx.accounts.escrow.withdrawal_start()
                && now < ctx.accounts.escrow.cancellation_start(),
            EscrowError::InvalidTime
        );

        // In a standard withdrawal, the rent recipient receives the entire rent amount, including the safety deposit,
        // because they initially covered the entire rent during order creation.

        common::escrow::withdraw(
            &ctx.accounts.escrow,
            ctx.bumps.escrow,
            &ctx.accounts.escrow_ata,
            &ctx.accounts.taker_ata,
            &ctx.accounts.mint,
            &ctx.accounts.token_program,
            &ctx.accounts.taker,
            &ctx.accounts.taker,
            secret,
        )
    }

    pub fn public_withdraw(ctx: Context<PublicWithdraw>, secret: [u8; 32]) -> Result<()> {
        let now = utils::get_current_timestamp()?;
        require!(
            now >= ctx.accounts.escrow.public_withdrawal_start()
                && now < ctx.accounts.escrow.cancellation_start(),
            EscrowError::InvalidTime
        );

        // In a public withdrawal, the rent recipient receives the rent minus the safety deposit
        // while the safety deposit is awarded to the payer who executed the public withdrawal

        common::escrow::withdraw(
            &ctx.accounts.escrow,
            ctx.bumps.escrow,
            &ctx.accounts.escrow_ata,
            &ctx.accounts.taker_ata,
            &ctx.accounts.mint,
            &ctx.accounts.token_program,
            &ctx.accounts.taker,
            &ctx.accounts.payer,
            secret,
        )
    }

    pub fn cancel_escrow(ctx: Context<CancelEscrow>) -> Result<()> {
        let now = utils::get_current_timestamp()?;
        require!(
            now >= ctx.accounts.escrow.cancellation_start(),
            EscrowError::InvalidTime
        );

        // In a standard cancel, the taker receives the entire rent amount, including the safety deposit,
        // because they initially covered the entire rent during escrow creation.

        common::escrow::cancel(
            &ctx.accounts.escrow,
            ctx.bumps.escrow,
            &ctx.accounts.escrow_ata,
            ctx.accounts.maker_ata.as_deref(),
            &ctx.accounts.mint,
            &ctx.accounts.token_program,
            &ctx.accounts.taker,
            &ctx.accounts.maker,
            &ctx.accounts.taker,
        )
    }

    pub fn public_cancel_escrow(ctx: Context<PublicCancelEscrow>) -> Result<()> {
        let now = utils::get_current_timestamp()?;
        require!(
            now >= ctx.accounts.escrow.public_cancellation_start,
            EscrowError::InvalidTime
        );

        common::escrow::cancel(
            &ctx.accounts.escrow,
            ctx.bumps.escrow,
            &ctx.accounts.escrow_ata,
            ctx.accounts.maker_ata.as_deref(),
            &ctx.accounts.mint,
            &ctx.accounts.token_program,
            &ctx.accounts.taker,
            &ctx.accounts.maker,
            &ctx.accounts.payer,
        )
    }

    pub fn rescue_funds_for_escrow(
        ctx: Context<RescueFundsForEscrow>,
        order_hash: [u8; 32],
        hashlock: [u8; 32],
        maker: Pubkey,
        token: Pubkey,
        amount: u64,
        safety_deposit: u64,
        rescue_start: u32,
        rescue_amount: u64,
    ) -> Result<()> {
        let taker_pubkey = ctx.accounts.taker.key();
        let seeds = [
            "escrow".as_bytes(),
            order_hash.as_ref(),
            hashlock.as_ref(),
            maker.as_ref(),
            taker_pubkey.as_ref(),
            token.as_ref(),
            &amount.to_be_bytes(),
            &safety_deposit.to_be_bytes(),
            &rescue_start.to_be_bytes(),
            &[ctx.bumps.escrow],
        ];

        common::escrow::rescue_funds(
            &ctx.accounts.escrow,
            rescue_start,
            &ctx.accounts.escrow_ata,
            &ctx.accounts.taker,
            &ctx.accounts.taker_ata,
            &ctx.accounts.mint,
            &ctx.accounts.token_program,
            rescue_amount,
            &seeds,
        )
    }

    pub fn rescue_funds_for_order(
        ctx: Context<RescueFundsForOrder>,
        order_hash: [u8; 32],
        hashlock: [u8; 32],
        order_creator: Pubkey,
        order_mint: Pubkey,
        order_amount: u64,
        safety_deposit: u64,
        rescue_start: u32,
        rescue_amount: u64,
    ) -> Result<()> {
        let seeds = [
            "order".as_bytes(),
            order_hash.as_ref(),
            hashlock.as_ref(),
            order_creator.as_ref(),
            order_mint.as_ref(),
            &order_amount.to_be_bytes(),
            &safety_deposit.to_be_bytes(),
            &rescue_start.to_be_bytes(),
            &[ctx.bumps.order],
        ];

        common::escrow::rescue_funds(
            &ctx.accounts.order,
            rescue_start,
            &ctx.accounts.order_ata,
            &ctx.accounts.resolver,
            &ctx.accounts.resolver_ata,
            &ctx.accounts.mint,
            &ctx.accounts.token_program,
            rescue_amount,
            &seeds,
        )
    }
}

#[derive(Accounts)]
#[instruction(order_hash: [u8; 32], hashlock: [u8; 32], amount: u64, safety_deposit: u64, finality_duration: u32, withdrawal_duration: u32, public_withdrawal_duration: u32, cancellation_duration: u32, rescue_start: u32)]
pub struct Create<'info> {
    #[account(
        mut, // Needed because this account transfers lamports if the token is native and to pay for the order creation
    )]
    creator: Signer<'info>,
    /// CHECK: check is not necessary as token is only used as a constraint to creator_ata and order
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = creator,
        associated_token::token_program = token_program
    )]
    /// Account to store creator's tokens (Optional if the token is native)
    creator_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
    /// Account to store order details
    #[account(
        init,
        payer = creator,
        space = constants::DISCRIMINATOR_BYTES + Order::INIT_SPACE,
        seeds = [
            "order".as_bytes(),
            order_hash.as_ref(),
            hashlock.as_ref(),
            creator.key().as_ref(),
            mint.key().as_ref(),
            amount.to_be_bytes().as_ref(),
            safety_deposit.to_be_bytes().as_ref(),
            rescue_start.to_be_bytes().as_ref(),
            ],
        bump,
    )]
    order: Box<Account<'info, Order>>,
    /// Account to store escrowed tokens
    #[account(
        init,
        payer = creator,
        associated_token::mint = mint,
        associated_token::authority = order,
        associated_token::token_program = token_program
    )]
    order_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(address = ASSOCIATED_TOKEN_PROGRAM_ID)]
    associated_token_program: Program<'info, AssociatedToken>,
    token_program: Interface<'info, TokenInterface>,
    rent: Sysvar<'info, Rent>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CreateEscrow<'info> {
    #[account(mut)]
    taker: Signer<'info>,
    #[account(
        seeds = [whitelist::RESOLVER_ACCESS_SEED, taker.key().as_ref()],
        bump = resolver_access.bump,
        seeds::program = whitelist::ID,
    )]
    resolver_access: Account<'info, whitelist::ResolverAccess>,
    #[account(
        mut, // Necessary because lamports will be transferred to this account when the order accounts are closed.
        constraint = maker.key() == order.creator @ EscrowError::InvalidAccount
    )]
    /// CHECK: this account is used only to receive rent for order and order_ata accounts
    maker: AccountInfo<'info>,
    /// CHECK: check is not necessary as token is only used as a constraint to creator_ata and order
    mint: Box<InterfaceAccount<'info, Mint>>,

    /// Account to store order details
    #[account(
        mut,
        seeds = [
            "order".as_bytes(),
            order.order_hash.as_ref(),
            order.hashlock.as_ref(),
            order.creator.as_ref(),
            order.token.key().as_ref(),
            order.amount.to_be_bytes().as_ref(),
            order.safety_deposit.to_be_bytes().as_ref(),
            order.rescue_start.to_be_bytes().as_ref(),
        ],
        bump,
    )]
    order: Box<Account<'info, Order>>,
    /// Account to store orders tokens
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = order,
        associated_token::token_program = token_program
    )]
    order_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Account to store escrow details
    #[account(
        init,
        payer = taker,
        space = constants::DISCRIMINATOR_BYTES + EscrowSrc::INIT_SPACE,
        seeds = [
            "escrow".as_bytes(),
            order.order_hash.as_ref(),
            order.hashlock.as_ref(),
            order.creator.as_ref(),
            taker.key().as_ref(),
            order.token.key().as_ref(),
            order.amount.to_be_bytes().as_ref(), // TODO: Must be replaced with the actual amount when partial fills are implemented.
            order.safety_deposit.to_be_bytes().as_ref(),
            order.rescue_start.to_be_bytes().as_ref(),
        ],
        bump,
    )]
    escrow: Box<Account<'info, EscrowSrc>>,
    /// Account to store escrowed tokens
    #[account(
        init,
        payer = taker,
        associated_token::mint = mint,
        associated_token::authority = escrow,
        associated_token::token_program = token_program
    )]
    escrow_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(address = ASSOCIATED_TOKEN_PROGRAM_ID)]
    associated_token_program: Program<'info, AssociatedToken>,
    token_program: Interface<'info, TokenInterface>,
    /// System program required for account initialization
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(
        mut, // Necessary because lamports will be transferred to this account when the escrow account is closed.
        constraint = taker.key() == escrow.taker @ EscrowError::InvalidAccount,
    )]
    taker: Signer<'info>,
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            escrow.maker.as_ref(),
            escrow.taker.as_ref(),
            mint.key().as_ref(),
            escrow.amount.to_be_bytes().as_ref(),
            escrow.safety_deposit.to_be_bytes().as_ref(),
            escrow.rescue_start.to_be_bytes().as_ref(),
        ],
        bump,
    )]
    escrow: Box<Account<'info, EscrowSrc>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = escrow,
        associated_token::token_program = token_program
    )]
    escrow_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = taker,
        associated_token::token_program = token_program
    )]
    taker_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    token_program: Interface<'info, TokenInterface>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct PublicWithdraw<'info> {
    /// CHECK: This account is used to check its pubkey to match the one stored in the escrow account
    #[account(
        mut, // Necessary because lamports will be transferred to this account when the escrow account is closed.
        constraint = taker.key() == escrow.taker @ EscrowError::InvalidAccount,
    )]
    taker: AccountInfo<'info>,
    #[account(mut)]
    payer: Signer<'info>,
    #[account(
        seeds = [whitelist::RESOLVER_ACCESS_SEED, payer.key().as_ref()],
        bump = resolver_access.bump,
        seeds::program = whitelist::ID,
    )]
    resolver_access: Account<'info, whitelist::ResolverAccess>,
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            escrow.maker.as_ref(),
            escrow.taker.as_ref(),
            mint.key().as_ref(),
            escrow.amount.to_be_bytes().as_ref(),
            escrow.safety_deposit.to_be_bytes().as_ref(),
            escrow.rescue_start.to_be_bytes().as_ref(),
        ],
        bump,
    )]
    escrow: Box<Account<'info, EscrowSrc>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = escrow,
        associated_token::token_program = token_program
    )]
    escrow_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = taker,
        associated_token::token_program = token_program
    )]
    taker_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    token_program: Interface<'info, TokenInterface>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CancelEscrow<'info> {
    #[account(
        mut, // Needed because this account receives lamports from rent
        constraint = taker.key() == escrow.taker @ EscrowError::InvalidAccount
    )]
    taker: Signer<'info>,
    /// CHECK: this account is used only to receive lamports and to check its pubkey to match the one stored in the escrow account
    #[account(
        mut, // Needed because this account receives lamports if the token is native
        constraint = maker.key() == escrow.maker @ EscrowError::InvalidAccount
    )]
    maker: AccountInfo<'info>,
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            escrow.maker.as_ref(),
            taker.key().as_ref(),
            escrow.token.key().as_ref(),
            escrow.amount.to_be_bytes().as_ref(), // TODO: Must be replaced with the actual amount when partial fills are implemented.
            escrow.safety_deposit.to_be_bytes().as_ref(),
            escrow.rescue_start.to_be_bytes().as_ref(),
        ],
        bump,
    )]
    escrow: Box<Account<'info, EscrowSrc>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = escrow,
        associated_token::token_program = token_program
    )]
    escrow_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = maker,
        associated_token::token_program = token_program
    )]
    // Optional if the token is native
    maker_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
    token_program: Interface<'info, TokenInterface>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct PublicCancelEscrow<'info> {
    /// CHECK: this account is used only to receive lamports and to check its pubkey to match the one stored in the escrow account
    #[account(
        mut, // Needed because this account receives lamports from rent
        constraint = taker.key() == escrow.taker @ EscrowError::InvalidAccount
    )]
    taker: AccountInfo<'info>,
    #[account(
        mut, // Needed because this account receives lamports if the token is native
        constraint = maker.key() == escrow.maker @ EscrowError::InvalidAccount
    )]
    /// CHECK: this account is used only to receive lamports and to check its pubkey to match the one stored in the escrow account
    maker: AccountInfo<'info>,
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(mut)]
    payer: Signer<'info>,
    #[account(
        seeds = [whitelist::RESOLVER_ACCESS_SEED, payer.key().as_ref()],
        bump = resolver_access.bump,
        seeds::program = whitelist::ID,
    )]
    resolver_access: Account<'info, whitelist::ResolverAccess>,
    #[account(
        mut,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            escrow.maker.as_ref(),
            taker.key().as_ref(),
            escrow.token.key().as_ref(),
            escrow.amount.to_be_bytes().as_ref(), // TODO: Must be replaced with the actual amount when partial fills are implemented.
            escrow.safety_deposit.to_be_bytes().as_ref(),
            escrow.rescue_start.to_be_bytes().as_ref(),
        ],
        bump,
    )]
    escrow: Box<Account<'info, EscrowSrc>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = escrow,
        associated_token::token_program = token_program
    )]
    escrow_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = escrow.maker,
        associated_token::token_program = token_program
    )]
    // Optional if the token is native
    maker_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
    token_program: Interface<'info, TokenInterface>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(order_hash: [u8; 32], hashlock: [u8; 32], maker: Pubkey, token: Pubkey, amount: u64, safety_deposit: u64, rescue_start: u32)]
pub struct RescueFundsForEscrow<'info> {
    #[account(
        mut, // Needed because this account receives lamports from closed token account.
    )]
    taker: Signer<'info>,
    mint: Box<InterfaceAccount<'info, Mint>>,
    /// CHECK: We don't accept escrow as 'Account<'info, Escrow>' because it may be already closed at the time of rescue funds.
    #[account(
        seeds = [
            "escrow".as_bytes(),
            order_hash.as_ref(),
            hashlock.as_ref(),
            maker.as_ref(),
            taker.key().as_ref(),
            token.as_ref(),
            amount.to_be_bytes().as_ref(),
            safety_deposit.to_be_bytes().as_ref(),
            rescue_start.to_be_bytes().as_ref(),
        ],
        bump,
    )]
    escrow: AccountInfo<'info>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = escrow,
        associated_token::token_program = token_program
    )]
    escrow_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = taker,
        associated_token::token_program = token_program
    )]
    taker_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    token_program: Interface<'info, TokenInterface>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(order_hash: [u8; 32], hashlock: [u8; 32], order_creator: Pubkey, order_mint: Pubkey, order_amount: u64, safety_deposit: u64, rescue_start: u32)]
pub struct RescueFundsForOrder<'info> {
    #[account(
        mut, // Needed because this account receives lamports from closed token account.
    )]
    resolver: Signer<'info>,
    #[account(
        seeds = [whitelist::RESOLVER_ACCESS_SEED, resolver.key().as_ref()],
        bump = resolver_access.bump,
        seeds::program = whitelist::ID,
    )]
    resolver_access: Account<'info, whitelist::ResolverAccess>,
    mint: Box<InterfaceAccount<'info, Mint>>,
    /// CHECK: We don't accept order as 'Account<'info, Order>' because it may be already closed at the time of rescue funds.
    #[account(
        seeds = [
            "order".as_bytes(),
            order_hash.as_ref(),
            hashlock.as_ref(),
            order_creator.as_ref(),
            order_mint.as_ref(),
            order_amount.to_be_bytes().as_ref(),
            safety_deposit.to_be_bytes().as_ref(),
            rescue_start.to_be_bytes().as_ref(),
        ],
        bump,
    )]
    order: AccountInfo<'info>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = order,
        associated_token::token_program = token_program
    )]
    order_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = resolver,
        associated_token::token_program = token_program
    )]
    resolver_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    token_program: Interface<'info, TokenInterface>,
    system_program: Program<'info, System>,
}

#[account]
#[derive(InitSpace)]
pub struct Order {
    order_hash: [u8; 32],
    hashlock: [u8; 32],
    creator: Pubkey,
    token: Pubkey,
    amount: u64,
    safety_deposit: u64,
    finality_duration: u32,
    withdrawal_duration: u32,
    public_withdrawal_duration: u32,
    cancellation_duration: u32,
    rescue_start: u32,
    expiration_time: u32,
    asset_is_native: bool,
    dst_amount: u64,
    dutch_auction_data_hash: [u8; 32],
}

#[account]
#[derive(InitSpace)]
pub struct EscrowSrc {
    order_hash: [u8; 32],
    hashlock: [u8; 32],
    maker: Pubkey,
    taker: Pubkey,
    token: Pubkey,
    amount: u64,
    safety_deposit: u64,
    withdrawal_start: u32,
    public_withdrawal_start: u32,
    cancellation_start: u32,
    public_cancellation_start: u32,
    rescue_start: u32,
    asset_is_native: bool,
    dst_amount: u64,
}

impl EscrowBase for EscrowSrc {
    fn order_hash(&self) -> &[u8; 32] {
        &self.order_hash
    }

    fn hashlock(&self) -> &[u8; 32] {
        &self.hashlock
    }

    fn creator(&self) -> &Pubkey {
        &self.maker
    }

    fn recipient(&self) -> &Pubkey {
        &self.taker
    }

    fn token(&self) -> &Pubkey {
        &self.token
    }

    fn amount(&self) -> u64 {
        self.amount
    }

    fn safety_deposit(&self) -> u64 {
        self.safety_deposit
    }

    fn withdrawal_start(&self) -> u32 {
        self.withdrawal_start
    }

    fn public_withdrawal_start(&self) -> u32 {
        self.public_withdrawal_start
    }

    fn cancellation_start(&self) -> u32 {
        self.cancellation_start
    }

    fn rescue_start(&self) -> u32 {
        self.rescue_start
    }

    fn asset_is_native(&self) -> bool {
        self.asset_is_native
    }
}

fn get_dst_amount(dst_amount: u64, data: &AuctionData) -> Result<u64> {
    let rate_bump = calculate_rate_bump(Clock::get()?.unix_timestamp as u64, data);
    let result = dst_amount
        .mul_div_ceil(constants::BASE_1E5 + rate_bump, constants::BASE_1E5)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    Ok(result)
}
