use anchor_lang::prelude::*;
use anchor_spl::associated_token::{AssociatedToken, ID as ASSOCIATED_TOKEN_PROGRAM_ID};
use anchor_spl::token::spl_token::native_mint::ID as NATIVE_MINT;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};
pub use common::constants;
use common::{
    error::EscrowError,
    escrow::{process_payout, uni_transfer, FeePayout, UniTransferParams},
    timelocks::{Stage, Timelocks},
    utils::get_current_timestamp,
};
use primitive_types::U256;

mod utils;

declare_id!("GveV3ToLhvRmeq1Fyg3BMkNetZuG9pZEp4uBGWLrTjve");

#[program]
pub mod cross_chain_escrow_dst {

    use super::*;

    pub fn create(
        ctx: Context<Create>,
        order_hash: [u8; 32],
        hashlock: [u8; 32],
        amount: u64,
        safety_deposit: u64,
        recipient: Pubkey,
        timelocks: [u64; 4],
        src_cancellation_timestamp: u32,
        asset_is_native: bool,
        fees: FeeParams,
    ) -> Result<()> {
        let updated_timelocks =
            Timelocks(U256(timelocks)).set_deployed_at(get_current_timestamp()?);
        let cancellation_start = updated_timelocks.get(Stage::DstCancellation)?;

        require!(
            cancellation_start <= src_cancellation_timestamp,
            EscrowError::InvalidCreationTime
        );

        // TODO: Verify that safety_deposit is enough to cover public_withdraw and public_cancel methods
        require!(
            amount != 0 && safety_deposit != 0,
            EscrowError::ZeroAmountOrDeposit
        );

        // The fees are taken from the escrowed amount on withdrawal, so they must leave something for the recipient
        let total_fees = fees
            .protocol_fee_amount
            .checked_add(fees.integrator_fee_amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        require!(total_fees < amount, EscrowError::InvalidFeeAmounts);

        // Verify that safety_deposit is less than escrow rent_exempt_reserve
        let rent_exempt_reserve =
            Rent::get()?.minimum_balance(EscrowDst::INIT_SPACE + constants::DISCRIMINATOR_BYTES);
        require!(
            safety_deposit <= rent_exempt_reserve,
            EscrowError::SafetyDepositTooLarge
        );

        require!(
            ctx.accounts.mint.key() == NATIVE_MINT || !asset_is_native,
            EscrowError::InconsistentNativeTrait
        );

        // Check if token is native (SOL)
        if asset_is_native {
            // Transfer native tokens from creator to escrow_ata. Wrapping is not required
            // because the protocol must return the tokens in the same form in which they were received.
            // Therefore, there is no point in performing unnecessary wrapping/unwrapping of tokens.
            uni_transfer(
                &UniTransferParams::NativeTransfer {
                    from: ctx.accounts.creator.to_account_info(),
                    to: ctx.accounts.escrow_ata.to_account_info(),
                    amount,
                    program: ctx.accounts.system_program.clone(),
                },
                None,
            )?;
        } else {
            // Transfer SPL tokens (WSOL included)
            uni_transfer(
                &UniTransferParams::TokenTransfer {
                    from: ctx
                        .accounts
                        .creator_ata
                        .as_ref()
                        .ok_or(EscrowError::MissingCreatorAta)?
                        .to_account_info(),
                    authority: ctx.accounts.creator.to_account_info(),
                    to: ctx.accounts.escrow_ata.to_account_info(),
                    mint: *ctx.accounts.mint.clone(),
                    amount,
                    program: ctx.accounts.token_program.clone(),
                },
                None,
            )?;
        }

        ctx.accounts.escrow.set_inner(EscrowDst {
            order_hash,
            hashlock,
            creator: ctx.accounts.creator.key(),
            recipient,
            token: ctx.accounts.mint.key(),
            amount,
            safety_deposit,
            timelocks: updated_timelocks.get_timelocks(),
            asset_is_native,
            fees,
            bump: ctx.bumps.escrow,
        });

        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>, secret: [u8; 32]) -> Result<()> {
        let now = get_current_timestamp()?;
        let timelocks = Timelocks(U256(ctx.accounts.escrow.timelocks));
        require!(
            now >= timelocks.get(Stage::DstWithdrawal)?
                && now < timelocks.get(Stage::DstCancellation)?,
            EscrowError::InvalidTime
        );

        // In a standard withdrawal, the creator receives the entire rent amount, including the safety deposit,
        // because they initially covered the entire rent during escrow creation.

        let fees = get_fee_payouts(
            &ctx.accounts.escrow.fees,
            ctx.accounts.protocol_fee_recipient.as_deref(),
            ctx.accounts.protocol_fee_recipient_ata.as_deref(),
            ctx.accounts.integrator_fee_recipient.as_deref(),
            ctx.accounts.integrator_fee_recipient_ata.as_deref(),
        );

        utils::withdraw(
            &ctx.accounts.escrow,
            ctx.accounts.escrow.bump,
            &ctx.accounts.escrow_ata,
            &ctx.accounts.recipient,
            ctx.accounts.recipient_ata.as_deref(),
            &ctx.accounts.mint,
            &ctx.accounts.token_program,
            &ctx.accounts.creator,
            &ctx.accounts.creator,
            secret,
            &fees,
        )
    }

    pub fn public_withdraw(ctx: Context<PublicWithdraw>, secret: [u8; 32]) -> Result<()> {
        let now = get_current_timestamp()?;
        let timelocks = Timelocks(U256(ctx.accounts.escrow.timelocks));
        require!(
            now >= timelocks.get(Stage::DstPublicWithdrawal)?
                && now < timelocks.get(Stage::DstCancellation)?,
            EscrowError::InvalidTime
        );

        // In a public withdrawal, the creator receives the rent minus the safety deposit
        // while the safety deposit is awarded to the payer who executed the public withdrawal

        let fees = get_fee_payouts(
            &ctx.accounts.escrow.fees,
            ctx.accounts.protocol_fee_recipient.as_deref(),
            ctx.accounts.protocol_fee_recipient_ata.as_deref(),
            ctx.accounts.integrator_fee_recipient.as_deref(),
            ctx.accounts.integrator_fee_recipient_ata.as_deref(),
        );

        utils::withdraw(
            &ctx.accounts.escrow,
            ctx.accounts.escrow.bump,
            &ctx.accounts.escrow_ata,
            &ctx.accounts.recipient,
            ctx.accounts.recipient_ata.as_deref(),
            &ctx.accounts.mint,
            &ctx.accounts.token_program,
            &ctx.accounts.creator,
            &ctx.accounts.payer,
            secret,
            &fees,
        )
    }

    pub fn cancel(ctx: Context<Cancel>) -> Result<()> {
        let now = get_current_timestamp()?;
        let timelocks = Timelocks(U256(ctx.accounts.escrow.timelocks));
        require!(
            now >= timelocks.get(Stage::DstCancellation)?,
            EscrowError::InvalidTime
        );

        let seeds = [
            "escrow".as_bytes(),
            &ctx.accounts.escrow.order_hash,
            &ctx.accounts.escrow.hashlock,
            ctx.accounts.escrow.creator.as_ref(),
            &ctx.accounts.escrow.amount.to_be_bytes(),
            &[ctx.accounts.escrow.bump],
        ];

        // On cancellation the whole amount, fees included, goes back to the creator
        process_payout(
            &ctx.accounts.mint,
            ctx.accounts.escrow.asset_is_native,
            ctx.accounts.escrow.amount,
            &ctx.accounts.escrow.to_account_info(),
            &ctx.accounts.escrow_ata,
            &ctx.accounts.creator,
            ctx.accounts.creator_ata.as_deref(),
            &ctx.accounts.creator,
            seeds,
            &ctx.accounts.token_program,
            &[],
        )?;

        Ok(())
    }

    pub fn rescue_funds(
        ctx: Context<RescueFunds>,
        order_hash: [u8; 32],
        hashlock: [u8; 32],
        escrow_amount: u64,
        rescue_amount: u64,
    ) -> Result<()> {
        let creator_pubkey = ctx.accounts.creator.key();

        let rescue_start = if !ctx.accounts.escrow.data_is_empty() {
            let escrow_data =
                EscrowDst::try_deserialize(&mut &ctx.accounts.escrow.data.borrow()[..])?;
            Some(Timelocks(U256(escrow_data.timelocks)).rescue_start(constants::RESCUE_DELAY)?)
        } else {
            None
        };

        let seeds = [
            "escrow".as_bytes(),
            order_hash.as_ref(),
            hashlock.as_ref(),
            creator_pubkey.as_ref(),
            &escrow_amount.to_be_bytes(),
            &[ctx.bumps.escrow],
        ];

        common::escrow::rescue_funds(
            &ctx.accounts.escrow,
            rescue_start,
            &ctx.accounts.escrow_ata,
            &ctx.accounts.creator,
            &ctx.accounts.creator_ata,
            &ctx.accounts.mint,
            &ctx.accounts.token_program,
            rescue_amount,
            &seeds,
        )
    }
}

#[derive(Accounts)]
#[instruction(order_hash: [u8; 32], hashlock: [u8; 32], amount: u64)]
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
        space = constants::DISCRIMINATOR_BYTES + EscrowDst::INIT_SPACE,
        seeds = [
            "escrow".as_bytes(),
            order_hash.as_ref(),
            hashlock.as_ref(),
            creator.key().as_ref(),
            amount.to_be_bytes().as_ref(),
            ],
        bump,
    )]
    escrow: Box<Account<'info, EscrowDst>>,
    /// Account to store escrowed tokens
    #[account(
        init_if_needed,
        payer = creator,
        associated_token::mint = mint,
        associated_token::authority = escrow,
        associated_token::token_program = token_program
    )]
    escrow_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(address = ASSOCIATED_TOKEN_PROGRAM_ID)]
    associated_token_program: Program<'info, AssociatedToken>,
    token_program: Interface<'info, TokenInterface>,
    rent: Sysvar<'info, Rent>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(
        mut, // Needed because this account receives lamports (safety deposit and from closed accounts)
        constraint = creator.key() == escrow.creator @ EscrowError::InvalidAccount
    )]
    creator: Signer<'info>,
    /// CHECK: This account is used to check its pubkey to match the one stored in the escrow account
    #[account(
        mut, // Needed because this account receives lamports if asset is native
        constraint = recipient.key() == escrow.recipient @ EscrowError::InvalidAccount)]
    recipient: AccountInfo<'info>,
    #[account(constraint = mint.key() == escrow.token @ EscrowError::InvalidMint)]
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        close = creator,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            escrow.creator.key().as_ref(),
            escrow.amount.to_be_bytes().as_ref(),
        ],
        bump = escrow.bump,
    )]
    escrow: Box<Account<'info, EscrowDst>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = escrow,
        associated_token::token_program = token_program
    )]
    escrow_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        init_if_needed,
        payer = creator,
        associated_token::mint = mint,
        associated_token::authority = recipient,
        associated_token::token_program = token_program
    )]
    /// Optional if the token is native
    recipient_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
    /// CHECK: This account is used to check its pubkey to match the one stored in the escrow account
    #[account(
        mut, // Needed because this account receives lamports if asset is native
        constraint = protocol_fee_recipient.key() == escrow.fees.protocol_fee_recipient @ EscrowError::InvalidAccount
    )]
    /// Receives the protocol fee if the asset is native. Required if the protocol fee is not zero and the asset is native
    protocol_fee_recipient: Option<UncheckedAccount<'info>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = escrow.fees.protocol_fee_recipient,
        associated_token::token_program = token_program
    )]
    /// Receives the protocol fee if the asset is not native. Required if the protocol fee is not zero and the asset is not native
    protocol_fee_recipient_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
    /// CHECK: This account is used to check its pubkey to match the one stored in the escrow account
    #[account(
        mut, // Needed because this account receives lamports if asset is native
        constraint = integrator_fee_recipient.key() == escrow.fees.integrator_fee_recipient @ EscrowError::InvalidAccount
    )]
    /// Receives the integrator fee if the asset is native. Required if the integrator fee is not zero and the asset is native
    integrator_fee_recipient: Option<UncheckedAccount<'info>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = escrow.fees.integrator_fee_recipient,
        associated_token::token_program = token_program
    )]
    /// Receives the integrator fee if the asset is not native. Required if the integrator fee is not zero and the asset is not native
    integrator_fee_recipient_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
    associated_token_program: Program<'info, AssociatedToken>,
    token_program: Interface<'info, TokenInterface>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct PublicWithdraw<'info> {
    /// CHECK: This account is used as a destination for rent, and its key is verified against the escrow.creator field
    #[account(
        mut, // Needed because this account receives lamports (safety deposit and from closed accounts)
        constraint = creator.key() == escrow.creator @ EscrowError::InvalidAccount
    )]
    creator: AccountInfo<'info>,
    /// CHECK: This account is used to check its pubkey to match the one stored in the escrow account
    #[account(
        mut, // Needed because this account receives lamports if asset is native
        constraint = recipient.key() == escrow.recipient @ EscrowError::InvalidAccount)]
    recipient: AccountInfo<'info>,
    #[account(mut)]
    payer: Signer<'info>,
    #[account(
        seeds = [ID.as_ref(), payer.key().as_ref()],
        bump = resolver_access.bump,
        seeds::program = whitelist::ID,
    )]
    resolver_access: Account<'info, whitelist::ResolverAccess>,
    #[account(constraint = mint.key() == escrow.token @ EscrowError::InvalidMint)]
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        close = creator,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            escrow.creator.key().as_ref(),
            escrow.amount.to_be_bytes().as_ref(),
        ],
        bump = escrow.bump,
    )]
    escrow: Box<Account<'info, EscrowDst>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = escrow,
        associated_token::token_program = token_program
    )]
    escrow_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = mint,
        associated_token::authority = recipient,
        associated_token::token_program = token_program
    )]
    /// Optional if the token is native
    recipient_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
    /// CHECK: This account is used to check its pubkey to match the one stored in the escrow account
    #[account(
        mut, // Needed because this account receives lamports if asset is native
        constraint = protocol_fee_recipient.key() == escrow.fees.protocol_fee_recipient @ EscrowError::InvalidAccount
    )]
    /// Receives the protocol fee if the asset is native. Required if the protocol fee is not zero and the asset is native
    protocol_fee_recipient: Option<UncheckedAccount<'info>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = escrow.fees.protocol_fee_recipient,
        associated_token::token_program = token_program
    )]
    /// Receives the protocol fee if the asset is not native. Required if the protocol fee is not zero and the asset is not native
    protocol_fee_recipient_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
    /// CHECK: This account is used to check its pubkey to match the one stored in the escrow account
    #[account(
        mut, // Needed because this account receives lamports if asset is native
        constraint = integrator_fee_recipient.key() == escrow.fees.integrator_fee_recipient @ EscrowError::InvalidAccount
    )]
    /// Receives the integrator fee if the asset is native. Required if the integrator fee is not zero and the asset is native
    integrator_fee_recipient: Option<UncheckedAccount<'info>>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = escrow.fees.integrator_fee_recipient,
        associated_token::token_program = token_program
    )]
    /// Receives the integrator fee if the asset is not native. Required if the integrator fee is not zero and the asset is not native
    integrator_fee_recipient_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
    associated_token_program: Program<'info, AssociatedToken>,
    token_program: Interface<'info, TokenInterface>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Cancel<'info> {
    #[account(
        mut, // Needed because this account receives lamports (safety deposit and from closed accounts)
        constraint = creator.key() == escrow.creator @ EscrowError::InvalidAccount
    )]
    creator: Signer<'info>,
    #[account(constraint = mint.key() == escrow.token @ EscrowError::InvalidMint)]
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        close = creator,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            escrow.creator.key().as_ref(),
            escrow.amount.to_be_bytes().as_ref(),
        ],
        bump = escrow.bump,
    )]
    escrow: Box<Account<'info, EscrowDst>>,
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
        associated_token::authority = creator,
        associated_token::token_program = token_program
    )]
    // Optional if the token is native
    creator_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
    token_program: Interface<'info, TokenInterface>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(order_hash: [u8; 32], hashlock: [u8; 32], escrow_amount: u64)]
pub struct RescueFunds<'info> {
    #[account(
        mut, // Needed because this account receives lamports from closed token account.
    )]
    creator: Signer<'info>,
    /// CHECK: This account is used to check its pubkey to match the one stored in the escrow account seeds
    recipient: AccountInfo<'info>,
    mint: Box<InterfaceAccount<'info, Mint>>,
    /// CHECK: We don't accept escrow as 'Account<'info, Escrow>' because it may be already closed at the time of rescue funds.
    #[account(
        seeds = [
            "escrow".as_bytes(),
            order_hash.as_ref(),
            hashlock.as_ref(),
            creator.key().as_ref(),
            escrow_amount.to_be_bytes().as_ref(),
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
        associated_token::authority = creator,
        associated_token::token_program = token_program
    )]
    creator_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    token_program: Interface<'info, TokenInterface>,
    system_program: Program<'info, System>,
}

#[account]
#[derive(InitSpace)]
pub struct EscrowDst {
    pub order_hash: [u8; 32],
    pub hashlock: [u8; 32],
    pub creator: Pubkey,
    pub recipient: Pubkey,
    pub token: Pubkey,
    pub asset_is_native: bool,
    pub amount: u64,
    pub safety_deposit: u64,
    pub timelocks: [u64; 4],
    pub fees: FeeParams,
    pub bump: u8,
}

/// Fees taken from `amount` on withdrawal. The fee amounts are computed on the source chain
/// when the source escrow is created and are forwarded here as is.
/// Mirrors the `parameters` field of the EVM escrow immutables
/// (`protocolFeeAmount`, `integratorFeeAmount`, `protocolFeeRecipient`, `integratorFeeRecipient`).
#[derive(
    AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq, InitSpace,
)]
pub struct FeeParams {
    pub protocol_fee_amount: u64,
    pub integrator_fee_amount: u64,
    pub protocol_fee_recipient: Pubkey,
    pub integrator_fee_recipient: Pubkey,
}

fn get_fee_payouts<'a, 'info>(
    fees: &FeeParams,
    protocol_fee_recipient: Option<&'a AccountInfo<'info>>,
    protocol_fee_recipient_ata: Option<&'a InterfaceAccount<'info, TokenAccount>>,
    integrator_fee_recipient: Option<&'a AccountInfo<'info>>,
    integrator_fee_recipient_ata: Option<&'a InterfaceAccount<'info, TokenAccount>>,
) -> [FeePayout<'a, 'info>; 2] {
    // Same payout order as EscrowDst._withdraw on EVM: integrator fee, protocol fee, then the recipient
    [
        FeePayout {
            amount: fees.integrator_fee_amount,
            recipient: integrator_fee_recipient,
            recipient_ata: integrator_fee_recipient_ata,
        },
        FeePayout {
            amount: fees.protocol_fee_amount,
            recipient: protocol_fee_recipient,
            recipient_ata: protocol_fee_recipient_ata,
        },
    ]
}
