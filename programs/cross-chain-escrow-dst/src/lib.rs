use anchor_lang::prelude::*;
use anchor_lang::solana_program::keccak::hash;
use anchor_spl::associated_token::{AssociatedToken, ID as ASSOCIATED_TOKEN_PROGRAM_ID};
use anchor_spl::token_interface::{
    close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
    TransferChecked,
};
pub use common::constants;
use common::error::EscrowError;
use common::escrow::{EscrowBase, EscrowType};
use common::utils;

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
        finality_duration: u32,
        withdrawal_duration: u32,
        public_withdrawal_duration: u32,
        src_cancellation_timestamp: u32,
        rescue_start: u32,
        asset_is_native: bool,
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

        require!(
            rescue_start >= now + common::constants::RESCUE_DELAY,
            EscrowError::InvalidRescueStart
        );

        require!(amount != 0 && safety_deposit != 0, EscrowError::ZeroAmountOrDeposit);

        let rent_exempt_reserve = Rent::get()?.minimum_balance(
            EscrowDst::INIT_SPACE + constants::DISCRIMINATOR_BYTES,
        );
        require!(safety_deposit <= rent_exempt_reserve, EscrowError::SafetyDepositTooLarge);

        require!(
            ctx.accounts.mint.key() == anchor_spl::token::spl_token::native_mint::ID
                || !asset_is_native,
            EscrowError::InconsistentNativeTrait
        );

        if asset_is_native {
            {
                let transfer_ctx = anchor_lang::system_program::Transfer {
                    from: ctx.accounts.creator.to_account_info(),
                    to: ctx.accounts.escrow_ata.to_account_info(),
                };

                let cpi_ctx = CpiContext::new(
                    ctx.accounts.system_program.to_account_info(),
                    transfer_ctx,
                );

                anchor_lang::system_program::transfer(cpi_ctx, amount)?;
            }

            if ctx.accounts.escrow.escrow_type() == EscrowType::Src {
                anchor_spl::token::sync_native(CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    anchor_spl::token::SyncNative {
                        account: ctx.accounts.escrow_ata.to_account_info(),
                    },
                ))?;
            }
        } else {
            {
                let ctx_t = anchor_spl::token_interface::TransferChecked {
                    from: ctx
                        .accounts
                        .creator_ata
                        .as_ref()
                        .ok_or(EscrowError::MissingCreatorAta)?
                        .to_account_info(),
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.escrow_ata.to_account_info(),
                    authority: ctx.accounts.creator.to_account_info(),
                };

                let cpi_ctx = CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    ctx_t,
                );

                anchor_spl::token_interface::transfer_checked(
                    cpi_ctx,
                    amount,
                    ctx.accounts.mint.decimals,
                )?;
            }
        }

        let escrow = &mut ctx.accounts.escrow;

        escrow.set_inner(EscrowDst {
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

    pub fn withdraw(ctx: Context<Withdraw>, secret: [u8; 32]) -> Result<()> {
        let now = utils::get_current_timestamp()?;
        require!(
            now >= ctx.accounts.escrow.withdrawal_start
                && now < ctx.accounts.escrow.cancellation_start,
            EscrowError::InvalidTime
        );

        // In a standard withdrawal, the creator receives the entire rent amount, including the safety deposit,
        // because they initially covered the entire rent during escrow creation.

        withdraw(
            &ctx.accounts.escrow,
            ctx.bumps.escrow,
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
            now >= ctx.accounts.escrow.public_withdrawal_start
                && now < ctx.accounts.escrow.cancellation_start,
            EscrowError::InvalidTime
        );

        // In a public withdrawal, the creator receives the rent minus the safety deposit
        // while the safety deposit is awarded to the payer who executed the public withdrawal

        withdraw(
            &ctx.accounts.escrow,
            ctx.bumps.escrow,
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
            now >= ctx.accounts.escrow.cancellation_start,
            EscrowError::InvalidTime
        );

        cancel(
            &ctx.accounts.escrow,
            ctx.bumps.escrow,
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
        let seeds = [
            "escrow".as_bytes(),
            order_hash.as_ref(),
            hashlock.as_ref(),
            creator_pubkey.as_ref(),
            recipient_pubkey.as_ref(),
            escrow_mint.as_ref(),
            &escrow_amount.to_be_bytes(),
            &safety_deposit.to_be_bytes(),
            &rescue_start.to_be_bytes(),
            &[ctx.bumps.escrow],
        ];

        rescue_funds(
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
            rescue_start.to_be_bytes().as_ref(),
            ],
        bump,
    )]
    escrow: Box<Account<'info, EscrowDst>>,
    /// Account to store escrowed tokens
    #[account(
        init,
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
            escrow.rescue_start.to_be_bytes().as_ref(),
        ],
        bump,
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
            escrow.rescue_start.to_be_bytes().as_ref(),
        ],
        bump,
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
            escrow.rescue_start.to_be_bytes().as_ref(),
        ],
        bump,
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
#[instruction(order_hash: [u8; 32], hashlock: [u8; 32], escrow_mint: Pubkey, escrow_amount: u64, safety_deposit: u64, rescue_start: u32)]
pub struct RescueFunds<'info> {
    #[account(
        mut, // Needed because this account receives lamports from closed token account.
    )]
    creator: Signer<'info>,
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
    withdrawal_start: u32,
    public_withdrawal_start: u32,
    cancellation_start: u32,
    rescue_start: u32,
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
        EscrowType::Dst
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

    {
        let ctx_t = anchor_spl::token_interface::TransferChecked {
            from: escrow_ata.to_account_info(),
            mint: mint.to_account_info(),
            to: recipient_ata.to_account_info(),
            authority: escrow.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            token_program.to_account_info(),
            ctx_t,
            &[seeds],
        );
        anchor_spl::token_interface::transfer_checked(
            cpi_ctx,
            rescue_amount,
            mint.decimals,
        )?;
    }

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
        let ctx_t = anchor_spl::token_interface::TransferChecked {
            from: escrow_ata.to_account_info(),
            mint: mint.to_account_info(),
            to: recipient_ata
                .ok_or(EscrowError::MissingRecipientAta)?
                .to_account_info(),
            authority: escrow.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            token_program.to_account_info(),
            ctx_t,
            &[&seeds],
        );
        anchor_spl::token_interface::transfer_checked(cpi_ctx, escrow_ata.amount, mint.decimals)?;

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
        let ctx_t = anchor_spl::token_interface::TransferChecked {
            from: escrow_ata.to_account_info(),
            mint: mint.to_account_info(),
            to: creator_ata
                .ok_or(EscrowError::MissingCreatorAta)?
                .to_account_info(),
            authority: escrow.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            token_program.to_account_info(),
            ctx_t,
            &[&seeds],
        );
        anchor_spl::token_interface::transfer_checked(cpi_ctx, escrow_ata.amount, mint.decimals)?;

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
