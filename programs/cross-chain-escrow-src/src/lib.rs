use anchor_lang::prelude::*;
use anchor_spl::associated_token::{AssociatedToken, ID as ASSOCIATED_TOKEN_PROGRAM_ID};
use anchor_spl::token::{Mint, Token, TokenAccount, ID as TOKEN_PROGRAM_ID};
pub use common::constants;
use common::error::EscrowError;
use common::escrow::EscrowBase;
use common::utils;

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
        recipient: Pubkey,
        finality_duration: u32,
        withdrawal_duration: u32,
        public_withdrawal_duration: u32,
        cancellation_duration: u32,
        rescue_start: u32,
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
        let public_cancellation_start = cancellation_start
            .checked_add(cancellation_duration)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        common::escrow::create(
            EscrowSrc::INIT_SPACE + constants::DISCRIMINATOR,
            &ctx.accounts.creator,
            &ctx.accounts.escrow_ata,
            &ctx.accounts.creator_ata,
            &ctx.accounts.token_program,
            amount,
            safety_deposit,
            rescue_start,
            now,
        )?;

        let escrow = &mut ctx.accounts.escrow;
        escrow.set_inner(EscrowSrc {
            order_hash,
            hashlock,
            creator: ctx.accounts.creator.key(),
            recipient,
            token: ctx.accounts.token.key(),
            amount,
            safety_deposit,
            withdrawal_start,
            public_withdrawal_start,
            cancellation_start,
            public_cancellation_start,
            rescue_start,
            rent_recipient: ctx.accounts.payer.key(),
        });

        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>, secret: [u8; 32]) -> Result<()> {
        let now = utils::get_current_timestamp()?;
        if now < ctx.accounts.escrow.withdrawal_start
            || now >= ctx.accounts.escrow.cancellation_start
        {
            return err!(EscrowError::InvalidTime);
        }

        // In a standard withdrawal, the rent recipient receives the entire rent amount, including the safety deposit,
        // because they initially covered the entire rent during escrow creation.

        common::escrow::withdraw(
            &ctx.accounts.escrow,
            ctx.bumps.escrow,
            &ctx.accounts.escrow_ata,
            &ctx.accounts.recipient_ata,
            &ctx.accounts.token_program,
            &ctx.accounts.rent_recipient,
            &ctx.accounts.rent_recipient,
            secret,
        )
    }

    pub fn public_withdraw(ctx: Context<PublicWithdraw>, secret: [u8; 32]) -> Result<()> {
        let now = utils::get_current_timestamp()?;
        if now < ctx.accounts.escrow.public_withdrawal_start
            || now >= ctx.accounts.escrow.cancellation_start
        {
            return err!(EscrowError::InvalidTime);
        }

        // In a public withdrawal, the rent recipient receives the rent minus the safety deposit
        // while the safety deposit is awarded to the payer who executed the public withdrawal

        common::escrow::withdraw(
            &ctx.accounts.escrow,
            ctx.bumps.escrow,
            &ctx.accounts.escrow_ata,
            &ctx.accounts.recipient_ata,
            &ctx.accounts.token_program,
            &ctx.accounts.rent_recipient,
            &ctx.accounts.payer,
            secret,
        )
    }

    pub fn cancel(ctx: Context<Cancel>) -> Result<()> {
        let now = utils::get_current_timestamp()?;
        if now < ctx.accounts.escrow.cancellation_start {
            return err!(EscrowError::InvalidTime);
        }

        // In a standard cancel, the rent recipient receives the entire rent amount, including the safety deposit,
        // because they initially covered the entire rent during escrow creation.
        common::escrow::cancel(
            &ctx.accounts.escrow,
            ctx.bumps.escrow,
            &ctx.accounts.escrow_ata,
            &ctx.accounts.creator_ata,
            &ctx.accounts.token_program,
            &ctx.accounts.rent_recipient,
            &ctx.accounts.rent_recipient,
        )
    }

    pub fn public_cancel(ctx: Context<PublicCancel>) -> Result<()> {
        let now = utils::get_current_timestamp()?;
        if now < ctx.accounts.escrow.public_cancellation_start {
            return err!(EscrowError::InvalidTime);
        }

        common::escrow::cancel(
            &ctx.accounts.escrow,
            ctx.bumps.escrow,
            &ctx.accounts.escrow_ata,
            &ctx.accounts.creator_ata,
            &ctx.accounts.token_program,
            &ctx.accounts.rent_recipient,
            &ctx.accounts.payer,
        )
    }

    pub fn rescue_funds(
        ctx: Context<RescueFunds>,
        order_hash: [u8; 32],
        hashlock: [u8; 32],
        escrow_creator: Pubkey,
        escrow_mint: Pubkey,
        escrow_amount: u64,
        safety_deposit: u64,
        rescue_start: u32,
        rescue_amount: u64,
    ) -> Result<()> {
        common::escrow::rescue_funds(
            &ctx.accounts.escrow,
            order_hash,
            hashlock,
            escrow_creator,
            escrow_mint,
            escrow_amount,
            safety_deposit,
            rescue_start,
            ctx.bumps.escrow,
            &ctx.accounts.escrow_ata,
            &ctx.accounts.recipient,
            &ctx.accounts.recipient_ata,
            &ctx.accounts.token_program,
            rescue_amount,
        )
    }
}

#[derive(Accounts)]
#[instruction(order_hash: [u8; 32], hashlock: [u8; 32], amount: u64, safety_deposit: u64, recipient: Pubkey, finality_duration: u32, withdrawal_duration: u32, public_withdrawal_duration: u32, cancellation_duration: u32, rescue_start: u32)]
pub struct Create<'info> {
    /// Pays for the creation of escrow account
    #[account(mut)]
    payer: Signer<'info>,
    /// Puts tokens into escrow
    creator: Signer<'info>,
    /// CHECK: check is not necessary as token is only used as a constraint to creator_ata and escrow_ata
    token: Box<Account<'info, Mint>>,
    #[account(
        mut,
        associated_token::mint = token,
        associated_token::authority = creator,
    )]
    /// Account to store creator's tokens
    creator_ata: Box<Account<'info, TokenAccount>>,
    /// Account to store escrow details
    #[account(
        init,
        payer = payer,
        space = constants::DISCRIMINATOR + EscrowSrc::INIT_SPACE,
        seeds = [
            "escrow".as_bytes(),
            order_hash.as_ref(),
            hashlock.as_ref(),
            creator.key().as_ref(),
            recipient.as_ref(),
            token.key().as_ref(),
            amount.to_be_bytes().as_ref(),
            safety_deposit.to_be_bytes().as_ref(),
            rescue_start.to_be_bytes().as_ref(),
            ],
        bump,
    )]
    escrow: Box<Account<'info, EscrowSrc>>,
    /// Account to store escrowed tokens
    #[account(
        init,
        payer = payer,
        associated_token::mint = token,
        associated_token::authority = escrow,
    )]
    escrow_ata: Box<Account<'info, TokenAccount>>,

    #[account(address = ASSOCIATED_TOKEN_PROGRAM_ID)]
    associated_token_program: Program<'info, AssociatedToken>,
    #[account(address = TOKEN_PROGRAM_ID)]
    token_program: Program<'info, Token>,
    rent: Sysvar<'info, Rent>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(constraint = recipient.key() == escrow.recipient @ EscrowError::InvalidAccount)]
    recipient: Signer<'info>,
    #[account(
        mut, // Needed because this account receives lamports (safety deposit and rent from closed accounts)
        constraint = rent_recipient.key() == escrow.rent_recipient @ EscrowError::InvalidAccount)]
    rent_recipient: AccountInfo<'info>,
    token: Box<Account<'info, Mint>>,
    #[account(
        mut,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            escrow.creator.as_ref(),
            escrow.recipient.key().as_ref(),
            token.key().as_ref(),
            escrow.amount.to_be_bytes().as_ref(),
            escrow.safety_deposit.to_be_bytes().as_ref(),
            escrow.rescue_start.to_be_bytes().as_ref(),
        ],
        bump,
    )]
    escrow: Box<Account<'info, EscrowSrc>>,
    #[account(
        mut,
        associated_token::mint = token,
        associated_token::authority = escrow,
    )]
    escrow_ata: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = token,
        associated_token::authority = recipient,
    )]
    recipient_ata: Box<Account<'info, TokenAccount>>,
    #[account(address = TOKEN_PROGRAM_ID)]
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct PublicWithdraw<'info> {
    /// CHECK: This account is used to check its pubkey to match the one stored in the escrow account
    #[account(constraint = recipient.key() == escrow.recipient @ EscrowError::InvalidAccount)]
    recipient: AccountInfo<'info>,
    #[account(
        mut, // Needed because this account receives lamports (safety deposit and from closed accounts)
        constraint = rent_recipient.key() == escrow.rent_recipient @ EscrowError::InvalidAccount)]
    rent_recipient: AccountInfo<'info>,
    #[account(mut)]
    payer: Signer<'info>,
    token: Box<Account<'info, Mint>>,
    #[account(
        mut,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            escrow.creator.as_ref(),
            escrow.recipient.key().as_ref(),
            token.key().as_ref(),
            escrow.amount.to_be_bytes().as_ref(),
            escrow.safety_deposit.to_be_bytes().as_ref(),
            escrow.rescue_start.to_be_bytes().as_ref(),
        ],
        bump,
    )]
    escrow: Box<Account<'info, EscrowSrc>>,
    #[account(
        mut,
        associated_token::mint = token,
        associated_token::authority = escrow,
    )]
    escrow_ata: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = token,
        associated_token::authority = recipient,
    )]
    recipient_ata: Box<Account<'info, TokenAccount>>,
    #[account(address = TOKEN_PROGRAM_ID)]
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Cancel<'info> {
    #[account(
        mut, // Needed because this account receives lamports (safety deposit and from closed accounts)
        constraint = creator.key() == escrow.creator @ EscrowError::InvalidAccount
    )]
    creator: AccountInfo<'info>,
    token: Box<Account<'info, Mint>>,
    #[account(
        mut,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            escrow.creator.as_ref(),
            escrow.recipient.key().as_ref(),
            token.key().as_ref(),
            escrow.amount.to_be_bytes().as_ref(),
            escrow.safety_deposit.to_be_bytes().as_ref(),
            escrow.rescue_start.to_be_bytes().as_ref(),
        ],
        bump,
    )]
    escrow: Box<Account<'info, EscrowSrc>>,
    #[account(
        mut,
        associated_token::mint = token,
        associated_token::authority = escrow,
    )]
    escrow_ata: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = token,
        associated_token::authority = creator,
    )]
    creator_ata: Box<Account<'info, TokenAccount>>,
    #[account(mut, constraint = rent_recipient.key() == escrow.rent_recipient @ EscrowError::InvalidAccount)]
    rent_recipient: AccountInfo<'info>,
    #[account(address = TOKEN_PROGRAM_ID)]
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct PublicCancel<'info> {
    /// CHECK: this account is used only to receive lampotrs and to check its pubkey to match the one stored in the escrow account
    #[account(
        mut, // Needed because this account receives lamports (safety deposit and from closed accounts)
        constraint = creator.key() == escrow.creator @ EscrowError::InvalidAccount
    )]
    creator: AccountInfo<'info>,
    token: Box<Account<'info, Mint>>,
    #[account(mut)]
    payer: Signer<'info>,
    #[account(
        mut,
        seeds = [
            "escrow".as_bytes(),
            escrow.order_hash.as_ref(),
            escrow.hashlock.as_ref(),
            escrow.creator.as_ref(),
            escrow.recipient.key().as_ref(),
            token.key().as_ref(),
            escrow.amount.to_be_bytes().as_ref(),
            escrow.safety_deposit.to_be_bytes().as_ref(),
            escrow.rescue_start.to_be_bytes().as_ref(),
        ],
        bump,
    )]
    escrow: Box<Account<'info, EscrowSrc>>,
    #[account(
        mut,
        associated_token::mint = token,
        associated_token::authority = escrow,
    )]
    escrow_ata: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = token,
        associated_token::authority = creator,
    )]
    creator_ata: Box<Account<'info, TokenAccount>>,
    #[account(mut, constraint = rent_recipient.key() == escrow.rent_recipient @ EscrowError::InvalidAccount)]
    rent_recipient: AccountInfo<'info>,
    #[account(address = TOKEN_PROGRAM_ID)]
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(order_hash: [u8; 32], hashlock: [u8; 32], escrow_creator: Pubkey, escrow_mint: Pubkey, escrow_amount: u64, safety_deposit: u64, rescue_start: u32)]
pub struct RescueFunds<'info> {
    #[account(
        mut, // Needed because this account receives lamports from closed token account.
    )]
    recipient: Signer<'info>,
    token: Box<Account<'info, Mint>>,
    /// CHECK: We don't accept escrow as 'Account<'info, Escrow>' because it may be already closed at the time of rescue funds.
    #[account(
        seeds = [
            "escrow".as_bytes(),
            order_hash.as_ref(),
            hashlock.as_ref(),
            escrow_creator.as_ref(),
            recipient.key().as_ref(),
            escrow_mint.as_ref(),
            escrow_amount.to_be_bytes().as_ref(),
            safety_deposit.to_be_bytes().as_ref(),
            rescue_start.to_be_bytes().as_ref(),
        ],
        bump,
    )]
    escrow: AccountInfo<'info>,
    #[account(
        mut,
        associated_token::mint = token,
        associated_token::authority = escrow,
    )]
    escrow_ata: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = token,
        associated_token::authority = recipient,
    )]
    recipient_ata: Box<Account<'info, TokenAccount>>,
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
}

#[account]
#[derive(InitSpace)]
pub struct EscrowSrc {
    order_hash: [u8; 32],
    hashlock: [u8; 32],
    creator: Pubkey,
    recipient: Pubkey,
    token: Pubkey,
    amount: u64,
    safety_deposit: u64,
    withdrawal_start: u32,
    public_withdrawal_start: u32,
    cancellation_start: u32,
    public_cancellation_start: u32,
    rescue_start: u32,
    rent_recipient: Pubkey,
}

impl EscrowBase for EscrowSrc {
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

    fn rent_recipient(&self) -> &Pubkey {
        &self.rent_recipient
    }
}
