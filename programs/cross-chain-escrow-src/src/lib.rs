use anchor_lang::prelude::*;
use anchor_lang::solana_program::keccak::hash;
use anchor_lang::solana_program::hash::hashv;
use anchor_spl::associated_token::{AssociatedToken, ID as ASSOCIATED_TOKEN_PROGRAM_ID};
use anchor_spl::token::spl_token::native_mint;
use anchor_spl::token_interface::{
    close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
    TransferChecked,
};
pub use auction::{calculate_premium, calculate_rate_bump, AuctionData};
pub use common::constants;
use common::error::EscrowError;
use common::escrow::{EscrowBase, EscrowType};
use common::utils;
use muldiv::MulDiv;

use crate::merkle_tree::MerkleProof;

pub mod auction;
pub mod merkle_tree;

declare_id!("6NwMYeUmigiMDjhYeYpbxC6Kc63NzZy1dfGd7fGcdkVS");

#[program]
pub mod cross_chain_escrow_src {

    use super::*;

    pub fn create(
        ctx: Context<Create>,
        order_hash: [u8; 32],
        hashlock: [u8; 32], // Root of merkle tree if partially filled
        amount: u64,
        parts_amount: u64,
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
        max_cancellation_premium: u64,
        cancellation_auction_duration: u32,
        allow_multiple_fills: bool,
    ) -> Result<()> {
        let now = utils::get_current_timestamp()?;

        require!(expiration_duration != 0, EscrowError::InvalidTime);

        let expiration_time = now
            .checked_add(expiration_duration)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        require!(
            ctx.accounts.order_ata.to_account_info().lamports() >= max_cancellation_premium,
            EscrowError::InvalidCancellationFee
        );

        require!(
            (allow_multiple_fills && parts_amount >= 2)
                || (!allow_multiple_fills && parts_amount == 1),
            EscrowError::InvalidPartsAmount
        );

        create(
            EscrowSrc::INIT_SPACE + constants::DISCRIMINATOR_BYTES,
            EscrowType::Src,
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
            remaining_amount: amount,
            parts_amount,
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
            max_cancellation_premium,
            cancellation_auction_duration,
            allow_multiple_fills,
        });

        Ok(())
    }

    pub fn create_escrow(
        ctx: Context<CreateEscrow>,
        amount: u64,
        dutch_auction_data: AuctionData,
        merkle_proof: Option<MerkleProof>,
    ) -> Result<()> {
        let order = &mut ctx.accounts.order;
        let escrow = &mut ctx.accounts.escrow;
        let now = utils::get_current_timestamp()?;
        require!(
            (order.allow_multiple_fills && amount <= order.remaining_amount)
                || (!order.allow_multiple_fills && amount == order.amount),
            EscrowError::InvalidAmount
        );

        require!(now < order.expiration_time, EscrowError::OrderHasExpired);

        let calculated_hash = hashv(&[&dutch_auction_data.try_to_vec()?]).to_bytes();
        require!(
            calculated_hash == order.dutch_auction_data_hash,
            EscrowError::DutchAuctionDataHashMismatch
        );

        let hashlock = match (order.allow_multiple_fills, &merkle_proof) {
            (true, Some(proof)) => {
                require!(
                    proof.verify(order.hashlock),
                    EscrowError::InvalidMerkleProof
                );
                require!(
                    is_valid_partial_fill(
                        amount,
                        order.remaining_amount,
                        order.amount,
                        order.parts_amount,
                        proof.index as u64,
                    ),
                    EscrowError::InvalidPartialFill
                );
                proof.hashed_secret
            }
            (false, None) => {
                // single fill, no merkle proof expected — OK
                order.hashlock
            }
            _ => return Err(EscrowError::InconsistentMerkleProofTrait.into()),
        };

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

        let order_seeds = [
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

        let amount_to_transfer =
            if order.remaining_amount == amount && ctx.accounts.order_ata.amount > amount {
                ctx.accounts.order_ata.amount
            } else {
                amount
            };

        uni_transfer(
            &UniTransferParams::TokenTransfer {
                from: ctx.accounts.order_ata.to_account_info(),
                authority: order.to_account_info(),
                to: ctx.accounts.escrow_ata.to_account_info(),
                mint: *ctx.accounts.mint.clone(),
                amount: amount_to_transfer,
                program: ctx.accounts.token_program.clone(),
            },
            Some(&[&order_seeds]),
        )?;

        escrow.set_inner(EscrowSrc {
            order_hash: order.order_hash,
            hashlock,
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

        if !order.allow_multiple_fills || order.remaining_amount == amount {
            // Close the order ATA
            close_account(CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                CloseAccount {
                    account: ctx.accounts.order_ata.to_account_info(),
                    destination: ctx.accounts.maker.to_account_info(),
                    authority: order.to_account_info(),
                },
                &[&order_seeds],
            ))?;

            // Close the order account
            order.close(ctx.accounts.maker.to_account_info())?;
        } else {
            order.remaining_amount -= amount;
        }

        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>, secret: [u8; 32]) -> Result<()> {
        let now = utils::get_current_timestamp()?;
        require!(
            now >= ctx.accounts.escrow.withdrawal_start()
                && now < ctx.accounts.escrow.cancellation_start(),
            EscrowError::InvalidTime
        );

        // In a standard withdrawal, the rent recipient receives the entire rent amount, including the safety deposit,
        // because they initially covered the entire rent during order creation.

        withdraw(
            &ctx.accounts.escrow,
            ctx.bumps.escrow,
            &ctx.accounts.escrow_ata,
            &ctx.accounts.taker,
            Some(&ctx.accounts.taker_ata),
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

        withdraw(
            &ctx.accounts.escrow,
            ctx.bumps.escrow,
            &ctx.accounts.escrow_ata,
            &ctx.accounts.taker,
            Some(&ctx.accounts.taker_ata),
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

        cancel(
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

    pub fn cancel_order(ctx: Context<CancelOrder>) -> Result<()> {
        let order = &ctx.accounts.order;
        require!(
            ctx.accounts.mint.key() == native_mint::id() || !order.asset_is_native,
            EscrowError::InconsistentNativeTrait
        );

        require!(
            order.asset_is_native == ctx.accounts.creator_ata.is_none(),
            EscrowError::InconsistentNativeTrait
        );

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

        if !order.asset_is_native {
            uni_transfer(
                &UniTransferParams::TokenTransfer {
                    from: ctx.accounts.order_ata.to_account_info(),
                    authority: order.to_account_info(),
                    to: ctx
                        .accounts
                        .creator_ata
                        .as_ref()
                        .ok_or(EscrowError::MissingCreatorAta)?
                        .to_account_info(),
                    mint: *ctx.accounts.mint.clone(),
                    amount: ctx.accounts.order_ata.amount,
                    program: ctx.accounts.token_program.clone(),
                },
                Some(&[&seeds]),
            )?;
        };

        close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            CloseAccount {
                account: ctx.accounts.order_ata.to_account_info(),
                destination: ctx.accounts.creator.to_account_info(),
                authority: order.to_account_info(),
            },
            &[&seeds],
        ))?;

        //Close the order account
        order.close(ctx.accounts.creator.to_account_info())
    }

    pub fn cancel_order_by_resolver(
        ctx: Context<CancelOrderbyResolver>,
        reward_limit: u64,
    ) -> Result<()> {
        let order = &ctx.accounts.order;
        let now = utils::get_current_timestamp()?;

        require!(now >= order.expiration_time, EscrowError::OrderNotExpired);

        require!(
            order.max_cancellation_premium > 0,
            EscrowError::CancelOrderByResolverIsForbidden
        );

        require!(
            order.asset_is_native == ctx.accounts.creator_ata.is_none(),
            EscrowError::InconsistentNativeTrait
        );

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

        // Return remaining src tokens back to maker
        if !order.asset_is_native {
            uni_transfer(
                &UniTransferParams::TokenTransfer {
                    from: ctx.accounts.order_ata.to_account_info(),
                    authority: order.to_account_info(),
                    to: ctx
                        .accounts
                        .creator_ata
                        .as_ref()
                        .ok_or(EscrowError::MissingCreatorAta)?
                        .to_account_info(),
                    mint: *ctx.accounts.mint.clone(),
                    amount: ctx.accounts.order_ata.amount,
                    program: ctx.accounts.token_program.clone(),
                },
                Some(&[&seeds]),
            )?;
        };

        let cancellation_premium = calculate_premium(
            now,
            order.expiration_time,
            order.cancellation_auction_duration,
            order.max_cancellation_premium,
        );

        let maker_amount = ctx.accounts.order_ata.to_account_info().lamports()
            - std::cmp::min(cancellation_premium, reward_limit);

        // Transfer all the remaining lamports to the resolver first
        close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            CloseAccount {
                account: ctx.accounts.order_ata.to_account_info(),
                destination: ctx.accounts.resolver.to_account_info(),
                authority: order.to_account_info(),
            },
            &[&seeds],
        ))?;

        // Transfer all lamports from the closed account, minus the cancellation premium, to the maker
        uni_transfer(
            &UniTransferParams::NativeTransfer {
                from: ctx.accounts.resolver.to_account_info(),
                to: ctx.accounts.creator.to_account_info(),
                amount: maker_amount,
                program: ctx.accounts.system_program.clone(),
            },
            None,
        )?;

        //Close the order account
        order.close(ctx.accounts.creator.to_account_info())
    }

    pub fn public_cancel_escrow(ctx: Context<PublicCancelEscrow>) -> Result<()> {
        let now = utils::get_current_timestamp()?;
        require!(
            now >= ctx.accounts.escrow.public_cancellation_start,
            EscrowError::InvalidTime
        );

        cancel(
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

        rescue_funds(
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

        rescue_funds(
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
#[instruction(order_hash: [u8; 32], hashlock: [u8; 32], amount: u64, parts_amount: u64, safety_deposit: u64, finality_duration: u32, withdrawal_duration: u32, public_withdrawal_duration: u32, cancellation_duration: u32, rescue_start: u32)]
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
#[instruction(amount: u64, dutch_auction_data: AuctionData, merkle_proof: Option<MerkleProof>)]
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
            &get_escrow_hashlock(
                order.hashlock,
                merkle_proof.clone()
            ),
            order.creator.as_ref(),
            taker.key().as_ref(),
            mint.key().as_ref(),
            amount.to_be_bytes().as_ref(),
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
        associated_token::authority = maker,
        associated_token::token_program = token_program
    )]
    // Optional if the token is native
    maker_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
    token_program: Interface<'info, TokenInterface>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CancelOrder<'info> {
    /// Account that created the order
    #[account(mut, signer)]
    creator: Signer<'info>,
    mint: Box<InterfaceAccount<'info, Mint>>,
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
        associated_token::authority = creator,
        associated_token::token_program = token_program
    )]
    // Optional if the token is native
    creator_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
    token_program: Interface<'info, TokenInterface>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CancelOrderbyResolver<'info> {
    /// Account that cancels the escrow
    #[account(mut, signer)]
    resolver: Signer<'info>,
    #[account(
        seeds = [whitelist::RESOLVER_ACCESS_SEED, resolver.key().as_ref()],
        bump = resolver_access.bump,
        seeds::program = whitelist::ID,
    )]
    resolver_access: Account<'info, whitelist::ResolverAccess>,
    /// CHECK: Currently only used for the token-authority check and to receive lamports if the token is native
    #[account(
        mut, // Needed because this account receives lamports if the token is native
        constraint = creator.key() == order.creator @ EscrowError::InvalidAccount
    )]
    creator: AccountInfo<'info>,
    mint: Box<InterfaceAccount<'info, Mint>>,
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
        associated_token::authority = creator,
        associated_token::token_program = token_program
    )]
    // Optional if the token is native
    creator_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
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
    remaining_amount: u64,
    parts_amount: u64,
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
    max_cancellation_premium: u64,
    cancellation_auction_duration: u32,
    allow_multiple_fills: bool,
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

    fn escrow_type(&self) -> EscrowType {
        EscrowType::Src
    }
}

fn get_dst_amount(dst_amount: u64, data: &AuctionData) -> Result<u64> {
    let rate_bump = calculate_rate_bump(Clock::get()?.unix_timestamp as u64, data);
    let result = dst_amount
        .mul_div_ceil(constants::BASE_1E5 + rate_bump, constants::BASE_1E5)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    Ok(result)
}

fn is_valid_partial_fill(
    making_amount: u64,
    remaining_making_amount: u64,
    order_making_amount: u64,
    parts_amount: u64,
    validated_index: u64,
) -> bool {
    let calculated_index = ((order_making_amount - remaining_making_amount + making_amount - 1)
        * parts_amount)
        / order_making_amount;

    if remaining_making_amount == making_amount {
        // If the order is filled to completion, a secret with index i + 1 must be used
        // where i is the index of the secret for the last part.
        return calculated_index + 1 == validated_index;
    } else if order_making_amount != remaining_making_amount {
        // Calculate the previous fill index only if this is not the first fill.
        let prev_calculated_index = ((order_making_amount - remaining_making_amount - 1)
            * parts_amount)
            / order_making_amount;
        if calculated_index == prev_calculated_index {
            return false;
        }
    }

    calculated_index == validated_index
}

pub fn get_escrow_hashlock(order_hash: [u8; 32], merkle_proof: Option<MerkleProof>) -> [u8; 32] {
    if let Some(merkle_proof) = merkle_proof {
        merkle_proof.hashed_secret
    } else {
        order_hash
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq)]
pub enum UniTransferParams<'info> {
    NativeTransfer {
        from: AccountInfo<'info>,
        to: AccountInfo<'info>,
        amount: u64,
        program: Program<'info, System>,
    },
    TokenTransfer {
        from: AccountInfo<'info>,
        authority: AccountInfo<'info>,
        to: AccountInfo<'info>,
        mint: InterfaceAccount<'info, Mint>,
        amount: u64,
        program: Interface<'info, TokenInterface>,
    },
}

fn uni_transfer(
    params: &UniTransferParams<'_>,
    signer_seeds: Option<&[&[&[u8]]]>,
) -> Result<()> {
    match params {
        UniTransferParams::NativeTransfer {
            from,
            to,
            amount,
            program,
        } => {
            let ctx = anchor_lang::system_program::Transfer {
                from: from.to_account_info(),
                to: to.to_account_info(),
            };
            let cpi_ctx = match signer_seeds {
                Some(seeds) => CpiContext::new_with_signer(program.to_account_info(), ctx, seeds),
                None => CpiContext::new(program.to_account_info(), ctx),
            };
            anchor_lang::system_program::transfer(cpi_ctx, *amount)
        }
        UniTransferParams::TokenTransfer {
            from,
            authority,
            to,
            mint,
            amount,
            program,
        } => {
            let ctx = TransferChecked {
                from: from.to_account_info(),
                mint: mint.to_account_info(),
                to: to.to_account_info(),
                authority: authority.to_account_info(),
            };
            let cpi_ctx = match signer_seeds {
                Some(seeds) => CpiContext::new_with_signer(program.to_account_info(), ctx, seeds),
                None => CpiContext::new(program.to_account_info(), ctx),
            };
            transfer_checked(cpi_ctx, *amount, mint.decimals)
        }
    }
}

fn close_escrow_account<'info, T>(
    escrow: &Account<'info, T>,
    safety_deposit_recipient: &AccountInfo<'info>,
    rent_recipient: &AccountInfo<'info>,
) -> Result<()>
where
    T: EscrowBase + AccountSerialize + AccountDeserialize + Clone,
{
    if rent_recipient.key() != safety_deposit_recipient.key() {
        let safety_deposit = escrow.safety_deposit();
        escrow.sub_lamports(safety_deposit)?;
        safety_deposit_recipient.add_lamports(safety_deposit)?;
    }
    escrow.close(rent_recipient.to_account_info())?;
    Ok(())
}

fn close_and_withdraw_native_ata<'info, T>(
    escrow: &Account<'info, T>,
    escrow_ata: &InterfaceAccount<'info, TokenAccount>,
    recipient: &AccountInfo<'info>,
    token_program: &Interface<'info, TokenInterface>,
    seeds: [&[u8]; 10],
) -> Result<()>
where
    T: EscrowBase + AccountSerialize + AccountDeserialize + Clone,
{
    close_account(CpiContext::new_with_signer(
        token_program.to_account_info(),
        CloseAccount {
            account: escrow_ata.to_account_info(),
            destination: escrow.to_account_info(),
            authority: escrow.to_account_info(),
        },
        &[&seeds],
    ))?;
    escrow.sub_lamports(escrow.amount())?;
    recipient.add_lamports(escrow.amount())?;
    Ok(())
}

fn create<'info>(
    escrow_size: usize,
    escrow_type: EscrowType,
    creator: &AccountInfo<'info>,
    asset_is_native: bool,
    escrow_ata: &InterfaceAccount<'info, TokenAccount>,
    creator_ata: Option<&InterfaceAccount<'info, TokenAccount>>,
    mint: &InterfaceAccount<'info, Mint>,
    token_program: &Interface<'info, TokenInterface>,
    sys_program: &Program<'info, System>,
    amount: u64,
    safety_deposit: u64,
    rescue_start: u32,
    now: u32,
) -> Result<()> {
    require!(rescue_start >= now + common::constants::RESCUE_DELAY, EscrowError::InvalidRescueStart);
    require!(amount != 0 && safety_deposit != 0, EscrowError::ZeroAmountOrDeposit);
    let rent_exempt_reserve = Rent::get()?.minimum_balance(escrow_size);
    require!(safety_deposit <= rent_exempt_reserve, EscrowError::SafetyDepositTooLarge);
    require!(mint.key() == anchor_spl::token::spl_token::native_mint::ID || !asset_is_native, EscrowError::InconsistentNativeTrait);
    if asset_is_native {
        uni_transfer(
            &UniTransferParams::NativeTransfer {
                from: creator.to_account_info(),
                to: escrow_ata.to_account_info(),
                amount,
                program: sys_program.clone(),
            },
            None,
        )?;
        if escrow_type == EscrowType::Src {
            anchor_spl::token::sync_native(CpiContext::new(
                token_program.to_account_info(),
                anchor_spl::token::SyncNative {
                    account: escrow_ata.to_account_info(),
                },
            ))?;
        }
    } else {
        uni_transfer(
            &UniTransferParams::TokenTransfer {
                from: creator_ata
                    .ok_or(EscrowError::MissingCreatorAta)?
                    .to_account_info(),
                authority: creator.to_account_info(),
                to: escrow_ata.to_account_info(),
                mint: mint.clone(),
                amount,
                program: token_program.clone(),
            },
            None,
        )?;
    }
    Ok(())
}

fn withdraw<'info, T>(
    escrow: &Account<'info, T>,
    escrow_bump: u8,
    escrow_ata: &InterfaceAccount<'info, TokenAccount>,
    recipient: &AccountInfo<'info>,
    recipient_ata: Option<&InterfaceAccount<'info, TokenAccount>>,
    mint: &InterfaceAccount<'info, Mint>,
    token_program: &Interface<'info, TokenInterface>,
    rent_recipient: &AccountInfo<'info>,
    safety_deposit_recipient: &AccountInfo<'info>,
    secret: [u8; 32],
) -> Result<()>
where
    T: EscrowBase + AccountSerialize + AccountDeserialize + Clone,
{
    require!(hash(&secret).to_bytes() == *escrow.hashlock(), EscrowError::InvalidSecret);
    let seeds = [
        "escrow".as_bytes(),
        escrow.order_hash(),
        escrow.hashlock(),
        escrow.creator().as_ref(),
        escrow.recipient().as_ref(),
        escrow.token().as_ref(),
        &escrow.amount().to_be_bytes(),
        &escrow.safety_deposit().to_be_bytes(),
        &escrow.rescue_start().to_be_bytes(),
        &[escrow_bump],
    ];
    if escrow.escrow_type() == EscrowType::Dst && escrow.asset_is_native() {
        close_and_withdraw_native_ata(escrow, escrow_ata, recipient, token_program, seeds)?;
    } else {
        uni_transfer(
            &UniTransferParams::TokenTransfer {
                from: escrow_ata.to_account_info(),
                authority: escrow.to_account_info(),
                to: recipient_ata
                    .ok_or(EscrowError::MissingRecipientAta)?
                    .to_account_info(),
                mint: mint.clone(),
                amount: escrow_ata.amount,
                program: token_program.clone(),
            },
            Some(&[&seeds]),
        )?;
        close_account(CpiContext::new_with_signer(
            token_program.to_account_info(),
            CloseAccount {
                account: escrow_ata.to_account_info(),
                destination: rent_recipient.to_account_info(),
                authority: escrow.to_account_info(),
            },
            &[&seeds],
        ))?;
    }
    close_escrow_account(escrow, safety_deposit_recipient, rent_recipient)?;
    Ok(())
}

fn cancel<'info, T>(
    escrow: &Account<'info, T>,
    escrow_bump: u8,
    escrow_ata: &InterfaceAccount<'info, TokenAccount>,
    creator_ata: Option<&InterfaceAccount<'info, TokenAccount>>,
    mint: &InterfaceAccount<'info, Mint>,
    token_program: &Interface<'info, TokenInterface>,
    rent_recipient: &AccountInfo<'info>,
    creator: &AccountInfo<'info>,
    safety_deposit_recipient: &AccountInfo<'info>,
) -> Result<()>
where
    T: EscrowBase + AccountSerialize + AccountDeserialize + Clone,
{
    let seeds = [
        "escrow".as_bytes(),
        escrow.order_hash(),
        escrow.hashlock(),
        escrow.creator().as_ref(),
        escrow.recipient().as_ref(),
        escrow.token().as_ref(),
        &escrow.amount().to_be_bytes(),
        &escrow.safety_deposit().to_be_bytes(),
        &escrow.rescue_start().to_be_bytes(),
        &[escrow_bump],
    ];
    if !escrow.asset_is_native() {
        uni_transfer(
            &UniTransferParams::TokenTransfer {
                from: escrow_ata.to_account_info(),
                authority: escrow.to_account_info(),
                to: creator_ata
                    .ok_or(EscrowError::MissingCreatorAta)?
                    .to_account_info(),
                mint: mint.clone(),
                amount: escrow_ata.amount,
                program: token_program.clone(),
            },
            Some(&[&seeds]),
        )?;
        close_account(CpiContext::new_with_signer(
            token_program.to_account_info(),
            CloseAccount {
                account: escrow_ata.to_account_info(),
                destination: rent_recipient.to_account_info(),
                authority: escrow.to_account_info(),
            },
            &[&seeds],
        ))?;
    } else {
        close_and_withdraw_native_ata(escrow, escrow_ata, creator, token_program, seeds)?;
    }
    close_escrow_account(escrow, safety_deposit_recipient, rent_recipient)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn rescue_funds<'info>(
    escrow: &AccountInfo<'info>,
    rescue_start: u32,
    escrow_ata: &InterfaceAccount<'info, TokenAccount>,
    recipient: &AccountInfo<'info>,
    recipient_ata: &InterfaceAccount<'info, TokenAccount>,
    mint: &InterfaceAccount<'info, Mint>,
    token_program: &Interface<'info, TokenInterface>,
    rescue_amount: u64,
    seeds: &[&[u8]],
) -> Result<()> {
    let now = common::utils::get_current_timestamp()?;
    require!(now >= rescue_start, EscrowError::InvalidTime);
    uni_transfer(
        &UniTransferParams::TokenTransfer {
            from: escrow_ata.to_account_info(),
            to: recipient_ata.to_account_info(),
            authority: escrow.to_account_info(),
            mint: mint.clone(),
            amount: rescue_amount,
            program: token_program.clone(),
        },
        Some(&[seeds]),
    )?;
    if rescue_amount == escrow_ata.amount {
        close_account(CpiContext::new_with_signer(
            token_program.to_account_info(),
            CloseAccount {
                account: escrow_ata.to_account_info(),
                destination: recipient.to_account_info(),
                authority: escrow.to_account_info(),
            },
            &[seeds],
        ))?;
    }
    Ok(())
}
