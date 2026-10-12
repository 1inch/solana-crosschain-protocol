use crate::merkle_tree::MerkleProof;
use anchor_lang::prelude::*;
use anchor_spl::associated_token::{AssociatedToken, ID as ASSOCIATED_TOKEN_PROGRAM_ID};
use anchor_spl::token::spl_token::native_mint::ID as NATIVE_MINT;
use anchor_spl::token_interface::{
    close_account, CloseAccount, Mint, TokenAccount, TokenInterface,
};
pub use auction::{calculate_premium, calculate_rate_bump, AuctionData};
pub use common::constants;
use common::{
    error::EscrowError,
    escrow::{uni_transfer, UniTransferParams},
    timelocks::{Stage, Timelocks},
    utils::get_current_timestamp,
};
use solana_keccak_hasher as keccak;

use primitive_types::U256;

pub mod auction;
pub mod merkle_tree;
pub mod utils;

declare_id!("2g4JDRMD7G3dK1PHmCnDAycKzd6e5sdhxqGBbs264zwz");

#[program]
pub mod cross_chain_escrow_src {

    use super::*;

    #[allow(clippy::too_many_arguments)]
    pub fn create(
        ctx: Context<Create>,
        hashlock: [u8; 32], // Root of merkle tree if partially filled
        amount: u64,
        safety_deposit: u64,
        timelocks: [u64; 4],
        expiration_time: u32,
        asset_is_native: bool,
        dst_amount: [u64; 4],
        dutch_auction_data_hash: [u8; 32],
        max_cancellation_premium: u64,
        cancellation_auction_duration: u32,
        allow_multiple_fills: bool,
        salt: u64,
        fee: FeeConfig,
        _dst_chain_params: DstChainParams,
    ) -> Result<()> {
        require!(
            ctx.accounts.order_ata.to_account_info().lamports() >= max_cancellation_premium,
            EscrowError::InvalidCancellationFee
        );

        if allow_multiple_fills {
            let parts_amount = u16::from_be_bytes([hashlock[0], hashlock[1]]);

            require!(parts_amount > 1, EscrowError::InvalidPartsAmount);
        }

        require!(
            u64::from(fee.integrator_share) <= constants::BASE_1E2,
            EscrowError::InvalidIntegratorShare
        );

        let now = get_current_timestamp()?;

        require!(now < expiration_time, EscrowError::OrderHasExpired);

        let order_hash = get_order_hash(
            hashlock,
            ctx.accounts.creator.key(),
            ctx.accounts.mint.key(),
            amount,
            safety_deposit,
            timelocks,
            expiration_time,
            asset_is_native,
            dst_amount,
            dutch_auction_data_hash,
            max_cancellation_premium,
            cancellation_auction_duration,
            allow_multiple_fills,
            salt,
            &fee,
        );

        // TODO: Verify that safety_deposit is enough to cover public_withdraw and public_cancel methods
        require!(
            amount != 0 && safety_deposit != 0,
            EscrowError::ZeroAmountOrDeposit
        );

        // Verify that safety_deposit is less than escrow rent_exempt_reserve
        let rent_exempt_reserve =
            Rent::get()?.minimum_balance(EscrowSrc::INIT_SPACE + constants::DISCRIMINATOR_BYTES);
        require!(
            safety_deposit <= rent_exempt_reserve,
            EscrowError::SafetyDepositTooLarge
        );

        require!(
            ctx.accounts.mint.key() == NATIVE_MINT || !asset_is_native,
            EscrowError::InconsistentNativeTrait
        );

        // Check if token is native (WSOL) and is expected to be wrapped
        if asset_is_native {
            // Transfer native tokens from creator to escrow_ata and wrap
            uni_transfer(
                &UniTransferParams::NativeTransfer {
                    from: ctx.accounts.creator.to_account_info(),
                    to: ctx.accounts.order_ata.to_account_info(),
                    amount,
                    program: ctx.accounts.system_program.clone(),
                },
                None,
            )?;

            anchor_spl::token::sync_native(CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                anchor_spl::token::SyncNative {
                    account: ctx.accounts.order_ata.to_account_info(),
                },
            ))?;
        } else {
            // Do SPL token transfer
            uni_transfer(
                &UniTransferParams::TokenTransfer {
                    from: ctx
                        .accounts
                        .creator_ata
                        .as_ref()
                        .ok_or(EscrowError::MissingCreatorAta)?
                        .to_account_info(),
                    authority: ctx.accounts.creator.to_account_info(),
                    to: ctx.accounts.order_ata.to_account_info(),
                    mint: *ctx.accounts.mint.clone(),
                    amount,
                    program: ctx.accounts.token_program.clone(),
                },
                None,
            )?;
        }

        let updated_timelocks = Timelocks(U256(timelocks)).set_deployed_at(now);

        ctx.accounts.order.set_inner(Order {
            order_hash,
            hashlock,
            creator: ctx.accounts.creator.key(),
            token: ctx.accounts.mint.key(),
            amount,
            remaining_amount: amount,
            safety_deposit,
            timelocks: updated_timelocks.get_timelocks(),
            expiration_time,
            asset_is_native,
            dst_amount,
            dutch_auction_data_hash,
            max_cancellation_premium,
            cancellation_auction_duration,
            allow_multiple_fills,
            fee,
            bump: ctx.bumps.order,
        });

        Ok(())
    }

    pub fn create_escrow(
        ctx: Context<CreateEscrow>,
        amount: u64,
        merkle_proof: Option<MerkleProof>,
        dutch_auction_data: AuctionData,
    ) -> Result<()> {
        let order = &mut ctx.accounts.order;

        require!(
            (order.allow_multiple_fills && amount <= order.remaining_amount)
                || (!order.allow_multiple_fills && amount == order.amount),
            EscrowError::InvalidAmount
        );

        let now = get_current_timestamp()?;

        require!(now < order.expiration_time, EscrowError::OrderHasExpired);

        let calculated_hash = keccak::hashv(&[&dutch_auction_data.try_to_vec()?]).to_bytes();

        require!(
            calculated_hash == order.dutch_auction_data_hash,
            EscrowError::DutchAuctionDataHashMismatch
        );

        require!(
            order.allow_multiple_fills == merkle_proof.is_some(),
            EscrowError::InconsistentMerkleProofTrait
        );

        let hashlock = if let Some(proof) = merkle_proof {
            require!(
                proof.process_proof()[2..] == order.hashlock[2..],
                EscrowError::InvalidMerkleProof
            );
            let parts_amount = u16::from_be_bytes([order.hashlock[0], order.hashlock[1]]);
            require!(
                is_valid_partial_fill(
                    amount,
                    order.remaining_amount,
                    order.amount,
                    parts_amount as u64,
                    proof.index,
                ),
                EscrowError::InvalidPartialFill
            );
            proof.hashed_secret
        } else {
            order.hashlock
        };

        let order_seeds = ["order".as_bytes(), &order.order_hash, &[order.bump]];

        let mut amount_to_transfer = amount;
        if order.remaining_amount == amount {
            // Transfer amount may be increased due to external transfers
            amount_to_transfer = ctx.accounts.order_ata.amount;
        }

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

        let updated_timelocks = Timelocks(U256(order.timelocks)).set_deployed_at(now);

        // The amount the maker should receive on the destination chain for this fill
        let dst_amount = get_dst_amount(
            U256(order.dst_amount)
                .checked_mul(U256::from(amount))
                .expect("Overflow during multiplication in dst_amount calculation")
                .checked_add(U256::from(order.amount - 1)) // Add (divisor - 1) for ceiling division
                .expect("Overflow when adding divisor - 1 for ceiling division")
                .checked_div(U256::from(order.amount))
                .expect("Division by zero or overflow during division in dst_amount calculation")
                .0,
            &dutch_auction_data,
        )?;

        // The fees are added on top of the maker's amount, the taker deposits the total on the destination chain
        let fee_amounts = get_fee_amounts(dst_amount, &order.fee)?;

        ctx.accounts.escrow.set_inner(EscrowSrc {
            order_hash: order.order_hash,
            hashlock,
            maker: order.creator,
            taker: ctx.accounts.taker.key(),
            token: order.token,
            amount,
            safety_deposit: order.safety_deposit,
            timelocks: updated_timelocks.get_timelocks(),
            asset_is_native: order.asset_is_native,
            dst_amount: fee_amounts.dst_amount,
            protocol_fee_amount: fee_amounts.protocol_fee_amount,
            integrator_fee_amount: fee_amounts.integrator_fee_amount,
            bump: ctx.bumps.escrow,
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
        let now = get_current_timestamp()?;

        let timelocks = Timelocks(U256(ctx.accounts.escrow.timelocks));
        require!(
            now >= timelocks.get(Stage::SrcWithdrawal)?
                && now < timelocks.get(Stage::SrcCancellation)?,
            EscrowError::InvalidTime
        );

        // In a standard withdrawal, the taker receives the entire rent amount, including the safety deposit,
        // because they initially covered the entire rent during escrow creation.

        utils::withdraw(
            &ctx.accounts.escrow,
            ctx.accounts.escrow.bump,
            &ctx.accounts.escrow_ata,
            &ctx.accounts.taker_ata,
            &ctx.accounts.mint,
            &ctx.accounts.token_program,
            &ctx.accounts.taker, // rent recipient
            &ctx.accounts.taker, // safety deposit recipient
            secret,
        )
    }

    pub fn public_withdraw(ctx: Context<PublicWithdraw>, secret: [u8; 32]) -> Result<()> {
        let now = get_current_timestamp()?;

        let timelocks = Timelocks(U256(ctx.accounts.escrow.timelocks));
        require!(
            now >= timelocks.get(Stage::SrcPublicWithdrawal)?
                && now < timelocks.get(Stage::SrcCancellation)?,
            EscrowError::InvalidTime
        );

        // In a public withdrawal, the taker receives the rent minus the safety deposit
        // while the safety deposit is awarded to the payer who executed the public withdrawal

        utils::withdraw(
            &ctx.accounts.escrow,
            ctx.accounts.escrow.bump,
            &ctx.accounts.escrow_ata,
            &ctx.accounts.taker_ata,
            &ctx.accounts.mint,
            &ctx.accounts.token_program,
            &ctx.accounts.taker, // rent recipient
            &ctx.accounts.payer, // safety deposit recipient
            secret,
        )
    }

    pub fn cancel_escrow(ctx: Context<CancelEscrow>) -> Result<()> {
        let now = get_current_timestamp()?;

        require!(
            now >= Timelocks(U256(ctx.accounts.escrow.timelocks)).get(Stage::SrcCancellation)?,
            EscrowError::InvalidTime
        );

        // In a standard cancel, the taker receives the entire rent amount, including the safety deposit,
        // because they initially covered the entire rent during escrow creation, while the maker
        // receives their tokens back to their initial ATA or wallet if the token is native.

        utils::cancel(
            &ctx.accounts.escrow,
            ctx.accounts.escrow.bump,
            &ctx.accounts.escrow_ata,
            ctx.accounts.maker_ata.as_deref(), // order creator ATA
            &ctx.accounts.mint,
            &ctx.accounts.token_program,
            &ctx.accounts.taker, // rent recipient
            &ctx.accounts.maker, // order creator
            &ctx.accounts.taker, // safety deposit recipient
        )
    }

    pub fn public_cancel_escrow(ctx: Context<PublicCancelEscrow>) -> Result<()> {
        let now = get_current_timestamp()?;
        require!(
            now >= Timelocks(U256(ctx.accounts.escrow.timelocks))
                .get(Stage::SrcPublicCancellation)?,
            EscrowError::InvalidTime
        );

        // In a public cancel, the taker receives the entire rent amount minus the safety deposit,
        // which is awarded to the payer who executed the public cancellation, while the maker
        // receives their tokens back to their initial ATA or wallet if the token is native.

        utils::cancel(
            &ctx.accounts.escrow,
            ctx.accounts.escrow.bump,
            &ctx.accounts.escrow_ata,
            ctx.accounts.maker_ata.as_deref(), // order creator ATA
            &ctx.accounts.mint,
            &ctx.accounts.token_program,
            &ctx.accounts.taker, // rent recipient
            &ctx.accounts.maker, // order creator
            &ctx.accounts.payer, // safety deposit recipient
        )
    }

    pub fn cancel_order(ctx: Context<CancelOrder>) -> Result<()> {
        let order = &ctx.accounts.order;

        require!(
            order.asset_is_native == ctx.accounts.creator_ata.is_none(),
            EscrowError::InconsistentNativeTrait
        );

        let seeds = ["order".as_bytes(), &order.order_hash, &[order.bump]];

        // In an order cancel, the maker receives the entire rent amount, including the safety deposit,
        // because they initially covered the entire rent during order creation, while also
        // receiving their tokens back to their initial ATA or wallet if the token is native

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
        ))
    }

    pub fn cancel_order_by_resolver(
        ctx: Context<CancelOrderbyResolver>,
        reward_limit: u64,
    ) -> Result<()> {
        let order = &ctx.accounts.order;
        let now = get_current_timestamp()?;

        require!(now >= order.expiration_time, EscrowError::OrderNotExpired);

        require!(
            order.max_cancellation_premium > 0,
            EscrowError::CancelOrderByResolverIsForbidden
        );

        require!(
            order.asset_is_native == ctx.accounts.creator_ata.is_none(),
            EscrowError::InconsistentNativeTrait
        );

        let seeds = ["order".as_bytes(), &order.order_hash, &[order.bump]];

        // Order creator receives the amount of tokens back to their initial ATA
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

        // The amount that the order maker will receive, which is the entire native
        // balance of the order ATA (or rent + wSOL) minus the cancellation premium
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

        // Transfer all lamports from the order ATA that resolver received,
        // minus the cancellation premium, to the maker
        uni_transfer(
            &UniTransferParams::NativeTransfer {
                from: ctx.accounts.resolver.to_account_info(),
                to: ctx.accounts.creator.to_account_info(),
                amount: maker_amount,
                program: ctx.accounts.system_program.clone(),
            },
            None,
        )
    }

    pub fn rescue_funds_for_escrow(
        ctx: Context<RescueFundsForEscrow>,
        order_hash: [u8; 32],
        hashlock: [u8; 32],
        amount: u64,
        rescue_amount: u64,
    ) -> Result<()> {
        let rescue_start = if !ctx.accounts.escrow.data_is_empty() {
            let escrow_data =
                EscrowSrc::try_deserialize(&mut &ctx.accounts.escrow.data.borrow()[..])?;
            Some(Timelocks(U256(escrow_data.timelocks)).rescue_start(constants::RESCUE_DELAY)?)
        } else {
            None
        };

        let taker_pubkey = ctx.accounts.taker.key();
        let seeds = [
            "escrow".as_bytes(),
            order_hash.as_ref(),
            hashlock.as_ref(),
            taker_pubkey.as_ref(),
            &amount.to_be_bytes(),
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

    #[allow(clippy::too_many_arguments)]
    pub fn rescue_funds_for_order(
        ctx: Context<RescueFundsForOrder>,
        hashlock: [u8; 32],
        maker: Pubkey,
        token: Pubkey,
        order_amount: u64,
        safety_deposit: u64,
        timelocks: [u64; 4],
        expiration_time: u32,
        asset_is_native: bool,
        dst_amount: [u64; 4],
        dutch_auction_data_hash: [u8; 32],
        max_cancellation_premium: u64,
        cancellation_auction_duration: u32,
        allow_multiple_fills: bool,
        salt: u64,
        fee: FeeConfig,
        rescue_amount: u64,
    ) -> Result<()> {
        let rescue_start = if !ctx.accounts.order.data_is_empty() {
            let order_data = Order::try_deserialize(&mut &ctx.accounts.order.data.borrow()[..])?;
            Some(Timelocks(U256(order_data.timelocks)).rescue_start(constants::RESCUE_DELAY)?)
        } else {
            None
        };

        let order_hash = get_order_hash(
            hashlock,
            maker,
            token,
            order_amount,
            safety_deposit,
            timelocks,
            expiration_time,
            asset_is_native,
            dst_amount,
            dutch_auction_data_hash,
            max_cancellation_premium,
            cancellation_auction_duration,
            allow_multiple_fills,
            salt,
            &fee,
        );

        let seeds = ["order".as_bytes(), order_hash.as_ref(), &[ctx.bumps.order]];

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
#[instruction(hashlock: [u8; 32],
              amount: u64,
              safety_deposit: u64,
              timelocks: [u64; 4],
              expiration_time: u32,
              asset_is_native: bool,
              dst_amount: [u64; 4],
              dutch_auction_data_hash: [u8; 32],
              max_cancellation_premium: u64,
              cancellation_auction_duration: u32,
              allow_multiple_fills: bool,
              salt: u64,
              fee: FeeConfig,
            )]
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
            &get_order_hash(
                hashlock,
                creator.key(),
                mint.key(),
                amount,
                safety_deposit,
                timelocks,
                expiration_time,
                asset_is_native,
                dst_amount,
                dutch_auction_data_hash,
                max_cancellation_premium,
                cancellation_auction_duration,
                allow_multiple_fills,
                salt,
                &fee,
            )
            ],
        bump,
    )]
    order: Box<Account<'info, Order>>,
    /// Account to store escrowed tokens
    #[account(
        init_if_needed,
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
#[instruction(amount: u64, merkle_proof: Option<MerkleProof>)]
pub struct CreateEscrow<'info> {
    #[account(mut)]
    taker: Signer<'info>,
    #[account(
        seeds = [ID.as_ref(), taker.key().as_ref()],
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
    #[account(
        constraint = mint.key() == order.token @ EscrowError::InvalidMint
    )]
    /// CHECK: check is not necessary as token is only used as a constraint to creator_ata and order
    mint: Box<InterfaceAccount<'info, Mint>>,

    /// Account to store order details
    #[account(
        mut,
        seeds = [
            "order".as_bytes(),
            order.order_hash.as_ref(),
        ],
        bump = order.bump,
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
            taker.key().as_ref(),
            amount.to_be_bytes().as_ref(),
        ],
        bump,
    )]
    escrow: Box<Account<'info, EscrowSrc>>,
    /// Account to store escrowed tokens
    #[account(
        init_if_needed,
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
    #[account(
        constraint = mint.key() == escrow.token @ EscrowError::InvalidMint
    )]
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        close = taker,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            escrow.taker.as_ref(),
            escrow.amount.to_be_bytes().as_ref(),
        ],
        bump = escrow.bump,
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
        seeds = [ID.as_ref(), payer.key().as_ref()],
        bump = resolver_access.bump,
        seeds::program = whitelist::ID,
    )]
    resolver_access: Account<'info, whitelist::ResolverAccess>,
    #[account(
        constraint = mint.key() == escrow.token @ EscrowError::InvalidMint
    )]
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        close = taker,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            escrow.taker.as_ref(),
            escrow.amount.to_be_bytes().as_ref(),
        ],
        bump = escrow.bump,
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
    #[account(
        constraint = mint.key() == escrow.token @ EscrowError::InvalidMint
    )]
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        close = taker,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            taker.key().as_ref(),
            escrow.amount.to_be_bytes().as_ref(),
        ],
        bump = escrow.bump,
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
    #[account(
        constraint = mint.key() == escrow.token @ EscrowError::InvalidMint
    )]
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(mut)]
    payer: Signer<'info>,
    #[account(
        seeds = [ID.as_ref(), payer.key().as_ref()],
        bump = resolver_access.bump,
        seeds::program = whitelist::ID,
    )]
    resolver_access: Account<'info, whitelist::ResolverAccess>,
    #[account(
        mut,
        close = taker,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            taker.key().as_ref(),
            escrow.amount.to_be_bytes().as_ref(),
        ],
        bump = escrow.bump,
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
pub struct CancelOrder<'info> {
    /// Account that created the order
    #[account(
        mut,
        constraint = creator.key() == order.creator @ EscrowError::InvalidAccount
    )]
    creator: Signer<'info>,
    #[account(
        constraint = mint.key() == order.token @ EscrowError::InvalidMint
    )]
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        close = creator,
        seeds = [
            "order".as_bytes(),
            order.order_hash.as_ref(),
        ],
        bump = order.bump,
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
    #[account(mut)]
    resolver: Signer<'info>,
    #[account(
        seeds = [ID.as_ref(), resolver.key().as_ref()],
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
    #[account(
        constraint = mint.key() == order.token @ EscrowError::InvalidMint
    )]
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        close = creator,
        seeds = [
            "order".as_bytes(),
            order.order_hash.as_ref(),
        ],
        bump = order.bump,
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
#[instruction(order_hash: [u8; 32], hashlock: [u8; 32], amount: u64)]
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
            taker.key().as_ref(),
            amount.to_be_bytes().as_ref(),
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
#[instruction(
        hashlock: [u8; 32],
        maker: Pubkey,
        token: Pubkey,
        order_amount: u64,
        safety_deposit: u64,
        timelocks: [u64; 4],
        expiration_time: u32,
        asset_is_native: bool,
        dst_amount: [u64; 4],
        dutch_auction_data_hash: [u8; 32],
        max_cancellation_premium: u64,
        cancellation_auction_duration: u32,
        allow_multiple_fills: bool,
        salt: u64,
        fee: FeeConfig,
)]
pub struct RescueFundsForOrder<'info> {
    #[account(
        mut, // Needed because this account receives lamports from closed token account.
    )]
    resolver: Signer<'info>,
    #[account(
        seeds = [ID.as_ref(), resolver.key().as_ref()],
        bump = resolver_access.bump,
        seeds::program = whitelist::ID,
    )]
    resolver_access: Account<'info, whitelist::ResolverAccess>,
    mint: Box<InterfaceAccount<'info, Mint>>,
    /// CHECK: We don't accept order as 'Account<'info, Order>' because it may be already closed at the time of rescue funds.
    #[account(
        seeds = [
            "order".as_bytes(),
            &get_order_hash(
                hashlock,
                maker,
                token.key(),
                order_amount,
                safety_deposit,
                timelocks,
                expiration_time,
                asset_is_native,
                dst_amount,
                dutch_auction_data_hash,
                max_cancellation_premium,
                cancellation_auction_duration,
                allow_multiple_fills,
                salt,
                &fee,
            )
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
    safety_deposit: u64,
    timelocks: [u64; 4],
    expiration_time: u32,
    asset_is_native: bool,
    dst_amount: [u64; 4],
    dutch_auction_data_hash: [u8; 32],
    max_cancellation_premium: u64,
    cancellation_auction_duration: u32,
    allow_multiple_fills: bool,
    fee: FeeConfig,
    bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct EscrowSrc {
    pub order_hash: [u8; 32],
    pub hashlock: [u8; 32],
    pub maker: Pubkey,
    pub taker: Pubkey,
    pub token: Pubkey,
    pub amount: u64,
    pub safety_deposit: u64,
    pub timelocks: [u64; 4],
    pub asset_is_native: bool,
    /// Total amount the taker has to deposit on the destination chain, fees included
    pub dst_amount: [u64; 4],
    /// Part of `dst_amount` that goes to the protocol fee recipient on the destination chain
    pub protocol_fee_amount: [u64; 4],
    /// Part of `dst_amount` that goes to the integrator fee recipient on the destination chain
    pub integrator_fee_amount: [u64; 4],
    pub bump: u8,
}

/// Fee configuration of an order. Mirrors the fee section of the EVM `FeeTaker` extension data.
#[derive(
    AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq, InitSpace,
)]
pub struct FeeConfig {
    /// Protocol (resolver) fee, 100% = `constants::BASE_1E5`
    pub protocol_fee: u16,
    /// Integrator fee, 100% = `constants::BASE_1E5`
    pub integrator_fee: u16,
    /// Share of the integrator fee that goes to the integrator, 100% = `constants::BASE_1E2`.
    /// The rest of the integrator fee goes to the protocol.
    pub integrator_share: u8,
}

/// Amounts the taker has to deposit on the destination chain for a fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeeAmounts {
    /// Total amount, fees included
    pub dst_amount: [u64; 4],
    pub protocol_fee_amount: [u64; 4],
    pub integrator_fee_amount: [u64; 4],
}

/// Adds the fees on top of `dst_amount` and splits them the same way the EVM `FeeTaker` does:
/// the total is `dst_amount * (1e5 + integrator_fee + protocol_fee) / 1e5` rounded up
/// (`AmountGetterWithFee._getTakingAmount`), the fee amounts are then taken from the total
/// (`FeeTaker._getFeeAmounts`). The maker receives `dst_amount - protocol_fee_amount - integrator_fee_amount`.
pub fn get_fee_amounts(dst_amount: [u64; 4], fee: &FeeConfig) -> Result<FeeAmounts> {
    if fee.protocol_fee == 0 && fee.integrator_fee == 0 {
        return Ok(FeeAmounts {
            dst_amount,
            protocol_fee_amount: [0; 4],
            integrator_fee_amount: [0; 4],
        });
    }

    let base = U256::from(constants::BASE_1E5);
    let integrator_fee = U256::from(fee.integrator_fee);
    let protocol_fee = U256::from(fee.protocol_fee);
    let denominator = base + integrator_fee + protocol_fee;

    let total = U256(dst_amount)
        .checked_mul(denominator)
        .ok_or(ProgramError::ArithmeticOverflow)?
        .checked_add(base - 1) // To ensure rounding up
        .ok_or(ProgramError::ArithmeticOverflow)?
        / base;

    let integrator_fee_total = total * integrator_fee / denominator;
    let integrator_fee_amount =
        integrator_fee_total * U256::from(fee.integrator_share) / U256::from(constants::BASE_1E2);
    let protocol_fee_amount =
        total * protocol_fee / denominator + integrator_fee_total - integrator_fee_amount;

    Ok(FeeAmounts {
        dst_amount: total.0,
        protocol_fee_amount: protocol_fee_amount.0,
        integrator_fee_amount: integrator_fee_amount.0,
    })
}

#[cfg(test)]
mod fee_amounts_tests {
    use super::*;

    fn amounts(dst_amount: u64, fee: FeeConfig) -> (u64, u64, u64) {
        let result = get_fee_amounts(U256::from(dst_amount).0, &fee).unwrap();
        (
            U256(result.dst_amount).as_u64(),
            U256(result.protocol_fee_amount).as_u64(),
            U256(result.integrator_fee_amount).as_u64(),
        )
    }

    #[test]
    fn zero_fees_keep_dst_amount() {
        assert_eq!(amounts(1000, FeeConfig::default()), (1000, 0, 0));
    }

    #[test]
    fn fees_are_added_on_top_and_split_like_evm_fee_taker() {
        let fee = FeeConfig {
            protocol_fee: 1000,
            integrator_fee: 500,
            integrator_share: 50,
        };
        // total = ceil(1000 * 101500 / 1e5) = 1015; integrator total = 5, integrator gets 2,
        // protocol gets 10 + (5 - 2) = 13; the maker still gets 1015 - 13 - 2 = 1000
        assert_eq!(amounts(1000, fee), (1015, 13, 2));
    }

    #[test]
    fn whole_integrator_fee_goes_to_integrator_with_full_share() {
        let fee = FeeConfig {
            protocol_fee: 0,
            integrator_fee: 10_000,
            integrator_share: 100,
        };
        assert_eq!(amounts(1_000_000, fee), (1_100_000, 0, 100_000));
    }

    #[test]
    fn whole_integrator_fee_goes_to_protocol_with_zero_share() {
        let fee = FeeConfig {
            protocol_fee: 0,
            integrator_fee: 10_000,
            integrator_share: 0,
        };
        assert_eq!(amounts(1_000_000, fee), (1_100_000, 100_000, 0));
    }

    #[test]
    fn total_is_rounded_up_and_maker_amount_is_never_below_dst_amount() {
        let fee = FeeConfig {
            protocol_fee: 1,
            integrator_fee: 1,
            integrator_share: 33,
        };
        // The total may exceed u64 for the largest inputs, so compare in U256
        for dst_amount in [1u64, 7, 99_999, 100_001, u64::MAX] {
            let result = get_fee_amounts(U256::from(dst_amount).0, &fee).unwrap();
            let maker_amount = U256(result.dst_amount)
                - U256(result.protocol_fee_amount)
                - U256(result.integrator_fee_amount);
            assert!(maker_amount >= U256::from(dst_amount));
        }
    }
}

fn get_dst_amount(dst_amount: [u64; 4], data: &AuctionData) -> Result<[u64; 4]> {
    let rate_bump = calculate_rate_bump(Clock::get()?.unix_timestamp as u64, data);
    let multiplier = constants::BASE_1E7 + rate_bump;

    let result = U256(dst_amount)
        .checked_mul(U256::from(multiplier))
        .expect("Overflow when multiplying destination amount with rate bump")
        .checked_add(U256::from(constants::BASE_1E7 - 1)) // To ensure rounding up
        .expect("Overflow when adding BASE_1E7 - 1")
        .checked_div(U256::from(constants::BASE_1E7))
        .expect("Overflow when dividing by BASE_1E7");
    Ok(result.0)
}

fn is_valid_partial_fill(
    making_amount: u64,
    remaining_making_amount: u64,
    order_making_amount: u64,
    parts_amount: u64,
    validated_index: u64,
) -> bool {
    let calculated_index = ((order_making_amount - remaining_making_amount + making_amount - 1)
        as u128
        * parts_amount as u128
        / order_making_amount as u128) as u64;

    if remaining_making_amount == making_amount {
        // If the order is filled to completion, a secret with index i + 1 must be used
        // where i is the index of the secret for the last part.
        return calculated_index + 1 == validated_index;
    } else if order_making_amount != remaining_making_amount {
        // Calculate the previous fill index only if this is not the first fill.
        let prev_calculated_index = ((order_making_amount - remaining_making_amount - 1) as u128
            * parts_amount as u128
            / order_making_amount as u128) as u64;
        if calculated_index == prev_calculated_index {
            return false;
        }
    }

    calculated_index == validated_index
}

pub fn get_escrow_hashlock(
    order_hashlock: [u8; 32],
    merkle_proof: Option<MerkleProof>,
) -> [u8; 32] {
    if let Some(merkle_proof) = merkle_proof {
        merkle_proof.hashed_secret
    } else {
        order_hashlock
    }
}

/// Parameters of the destination chain escrow. They are not stored on chain, the resolver
/// reads them from the `create` instruction data.
#[account]
pub struct DstChainParams {
    pub chain_id: u32,
    pub maker_address: [u8; 32],
    pub token: [u8; 32],
    pub safety_deposit: u128,
    /// Receives `EscrowSrc::protocol_fee_amount` on the destination chain
    pub protocol_fee_recipient: [u8; 32],
    /// Receives `EscrowSrc::integrator_fee_amount` on the destination chain
    pub integrator_fee_recipient: [u8; 32],
}

#[allow(clippy::too_many_arguments)]
fn get_order_hash(
    hashlock: [u8; 32],
    maker: Pubkey,
    token: Pubkey,
    order_amount: u64,
    safety_deposit: u64,
    timelocks: [u64; 4],
    expiration_time: u32,
    asset_is_native: bool,
    dst_amount: [u64; 4],
    dutch_auction_data_hash: [u8; 32],
    max_cancellation_premium: u64,
    cancellation_auction_duration: u32,
    allow_multiple_fills: bool,
    salt: u64,
    fee: &FeeConfig,
) -> [u8; 32] {
    keccak::hashv(&[
        &hashlock,
        maker.as_ref(),
        token.as_ref(),
        &order_amount.to_be_bytes(),
        &safety_deposit.to_be_bytes(),
        &timelocks.try_to_vec().unwrap(),
        &expiration_time.to_be_bytes(),
        &[asset_is_native as u8],
        &dst_amount.try_to_vec().unwrap(),
        dutch_auction_data_hash.as_ref(),
        &max_cancellation_premium.to_be_bytes(),
        &cancellation_auction_duration.to_be_bytes(),
        &[allow_multiple_fills as u8],
        &salt.to_be_bytes(),
        &fee.protocol_fee.to_be_bytes(),
        &fee.integrator_fee.to_be_bytes(),
        &[fee.integrator_share],
    ])
    .to_bytes()
}

#[cfg(test)]
mod tests {
    use super::is_valid_partial_fill;

    #[test]
    fn test_partial_fill_index_does_not_overflow_u64() {
        // 185M tokens with 9 decimals split into 100 parts: on the final fill
        // (filled amount - 1) * parts exceeds u64::MAX.
        let order = 185_000_000 * 1_000_000_000u64;
        let parts = 100;
        let part = order / parts;
        for i in 0..parts - 1 {
            let remaining = order - i * part;
            assert!(is_valid_partial_fill(part, remaining, order, parts, i));
            assert!(!is_valid_partial_fill(part, remaining, order, parts, i + 1));
        }
        // The fill that completes the order must use the extra secret with index `parts`.
        assert!(is_valid_partial_fill(part, part, order, parts, parts));
        assert!(!is_valid_partial_fill(part, part, order, parts, parts - 1));
    }

    #[test]
    fn test_partial_fill_index_with_max_amounts() {
        let max = u64::MAX;
        let parts = u64::from(u16::MAX);
        assert!(is_valid_partial_fill(max, max, max, parts, parts));
        assert!(is_valid_partial_fill(max - 1, max, max, parts, parts - 1));
        assert!(is_valid_partial_fill(1, 1, max, parts, parts));
    }
}
