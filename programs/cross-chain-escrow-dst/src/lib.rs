use anchor_lang::prelude::*;
use anchor_spl::associated_token::{AssociatedToken, ID as ASSOCIATED_TOKEN_PROGRAM_ID};
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};
pub use common::constants;
use common::{
    error::EscrowError,
    escrow::{EscrowBase, EscrowType},
    timelocks::{Stage, Timelocks},
    utils,
};
use primitive_types::U256;

declare_id!("B9SnVJbXNd6RFNxHqPkTvdr46YPT17xunemTQfDsCNzR");

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
    ) -> Result<()> {
        let updated_timelocks =
            Timelocks(U256(timelocks)).set_deployed_at(utils::get_current_timestamp()?);
        let cancellation_start = updated_timelocks.get(Stage::DstCancellation)?;

        require!(
            cancellation_start <= src_cancellation_timestamp,
            EscrowError::InvalidCreationTime
        );

        let rescue_start = updated_timelocks.rescue_start(constants::RESCUE_DELAY)?;

        common::escrow::create(
            EscrowDst::INIT_SPACE + constants::DISCRIMINATOR_BYTES,
            EscrowType::Dst,
            &ctx.accounts.creator,
            asset_is_native,
            &ctx.accounts.escrow_ata,
            ctx.accounts.creator_ata.as_deref(),
            &ctx.accounts.mint,
            &ctx.accounts.token_program,
            &ctx.accounts.system_program,
            amount,
            safety_deposit,
        )?;

        ctx.accounts.escrow.set_inner(EscrowDst {
            order_hash,
            hashlock,
            creator: ctx.accounts.creator.key(),
            recipient,
            token: ctx.accounts.mint.key(),
            amount,
            safety_deposit,
            timelocks: updated_timelocks.get_timelocks(),
            rescue_start,
            asset_is_native,
            bump: ctx.bumps.escrow,
        });

        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>, secret: [u8; 32]) -> Result<()> {
        let now = utils::get_current_timestamp()?;
        require!(
            now >= ctx.accounts.escrow.timelocks().get(Stage::DstWithdrawal)?
                && now
                    < ctx
                        .accounts
                        .escrow
                        .timelocks()
                        .get(Stage::DstCancellation)?,
            EscrowError::InvalidTime
        );

        // In a standard withdrawal, the creator receives the entire rent amount, including the safety deposit,
        // because they initially covered the entire rent during escrow creation.

        common::escrow::withdraw(
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
        )
    }

    pub fn public_withdraw(ctx: Context<PublicWithdraw>, secret: [u8; 32]) -> Result<()> {
        let now = utils::get_current_timestamp()?;
        require!(
            now >= ctx
                .accounts
                .escrow
                .timelocks()
                .get(Stage::DstPublicWithdrawal)?
                && now
                    < ctx
                        .accounts
                        .escrow
                        .timelocks()
                        .get(Stage::DstCancellation)?,
            EscrowError::InvalidTime
        );

        // In a public withdrawal, the creator receives the rent minus the safety deposit
        // while the safety deposit is awarded to the payer who executed the public withdrawal

        common::escrow::withdraw(
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
        )
    }

    pub fn cancel(ctx: Context<Cancel>) -> Result<()> {
        let now = utils::get_current_timestamp()?;
        require!(
            now >= ctx
                .accounts
                .escrow
                .timelocks()
                .get(Stage::DstCancellation)?,
            EscrowError::InvalidTime
        );

        common::escrow::cancel(
            &ctx.accounts.escrow,
            ctx.accounts.escrow.bump,
            &ctx.accounts.escrow_ata,
            ctx.accounts.creator_ata.as_deref(),
            &ctx.accounts.mint,
            &ctx.accounts.token_program,
            &ctx.accounts.creator,
            &ctx.accounts.creator,
            &ctx.accounts.creator,
        )
    }

    pub fn rescue_funds(
        ctx: Context<RescueFunds>,
        order_hash: [u8; 32],
        hashlock: [u8; 32],
        escrow_mint: Pubkey,
        escrow_amount: u64,
        safety_deposit: u64,
        rescue_start: u32,
        rescue_amount: u64,
    ) -> Result<()> {
        let recipient_pubkey = ctx.accounts.recipient.key();
        let creator_pubkey = ctx.accounts.creator.key();

        if !ctx.accounts.escrow.data_is_empty() {
            let escrow_data =
                EscrowDst::try_deserialize(&mut &ctx.accounts.escrow.data.borrow()[..])?;
            require!(
                rescue_start == escrow_data.rescue_start,
                EscrowError::InvalidRescueStart
            )
        }

        let seeds = [
            "escrow".as_bytes(),
            order_hash.as_ref(),
            hashlock.as_ref(),
            creator_pubkey.as_ref(),
            recipient_pubkey.as_ref(),
            escrow_mint.as_ref(),
            &escrow_amount.to_be_bytes(),
            &safety_deposit.to_be_bytes(),
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
#[instruction(order_hash: [u8; 32], hashlock: [u8; 32], amount: u64, safety_deposit: u64, recipient: Pubkey)]
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
            recipient.as_ref(),
            mint.key().as_ref(),
            amount.to_be_bytes().as_ref(),
            safety_deposit.to_be_bytes().as_ref(),
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
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            escrow.creator.as_ref(),
            escrow.recipient.key().as_ref(),
            mint.key().as_ref(),
            escrow.amount.to_be_bytes().as_ref(),
            escrow.safety_deposit.to_be_bytes().as_ref(),
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
        associated_token::authority = recipient,
        associated_token::token_program = token_program
    )]
    /// Optional if the token is native
    recipient_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
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
            escrow.creator.as_ref(),
            escrow.recipient.key().as_ref(),
            mint.key().as_ref(),
            escrow.amount.to_be_bytes().as_ref(),
            escrow.safety_deposit.to_be_bytes().as_ref(),
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
        associated_token::authority = recipient,
        associated_token::token_program = token_program
    )]
    /// Optional if the token is native
    recipient_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
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
    mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            escrow.creator.as_ref(),
            escrow.recipient.key().as_ref(),
            mint.key().as_ref(),
            escrow.amount.to_be_bytes().as_ref(),
            escrow.safety_deposit.to_be_bytes().as_ref(),
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
#[instruction(order_hash: [u8; 32], hashlock: [u8; 32], escrow_mint: Pubkey, escrow_amount: u64, safety_deposit: u64)]
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
            recipient.key().as_ref(),
            escrow_mint.as_ref(),
            escrow_amount.to_be_bytes().as_ref(),
            safety_deposit.to_be_bytes().as_ref(),
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
    order_hash: [u8; 32],
    hashlock: [u8; 32],
    creator: Pubkey,
    recipient: Pubkey,
    token: Pubkey,
    asset_is_native: bool,
    amount: u64,
    safety_deposit: u64,
    timelocks: [u64; 4],
    rescue_start: u32,
    bump: u8,
}

impl EscrowBase for EscrowDst {
    fn order_hash(&self) -> &[u8; 32] {
        &self.order_hash
    }

    fn hashlock(&self) -> &[u8; 32] {
        &self.hashlock
    }

    fn creator(&self) -> &Pubkey {
        &self.creator
    }

    fn recipient(&self) -> &Pubkey {
        &self.recipient
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

    fn timelocks(&self) -> Timelocks {
        Timelocks(U256(self.timelocks))
    }

    fn rescue_start(&self) -> u32 {
        self.rescue_start
    }

    fn asset_is_native(&self) -> bool {
        self.asset_is_native
    }

    fn escrow_type(&self) -> EscrowType {
        EscrowType::Dst
    }
}
