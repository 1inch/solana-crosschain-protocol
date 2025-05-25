use anchor_lang::solana_program::keccak::hash;
use anchor_lang::{prelude::*, system_program};
use anchor_spl::token::{spl_token::native_mint::ID as NATIVE_MINT, Token, TokenAccount};

use crate::constants::RESCUE_DELAY;
use crate::error::EscrowError;
use crate::utils;

pub trait EscrowBase {
    fn order_hash(&self) -> &[u8; 32];

    fn hashlock(&self) -> &[u8; 32];

    fn creator(&self) -> &Pubkey;

    fn recipient(&self) -> &Pubkey;

    fn token(&self) -> &Pubkey;

    fn amount(&self) -> u64;

    fn safety_deposit(&self) -> u64;

    fn withdrawal_start(&self) -> u32;

    fn public_withdrawal_start(&self) -> u32;

    fn cancellation_start(&self) -> u32;

    fn rescue_start(&self) -> u32;

    fn rent_recipient(&self) -> &Pubkey;
}

pub fn create<'info>(
    escrow_size: usize,
    creator: &AccountInfo<'info>,
    escrow_ata: &Account<'info, TokenAccount>,
    creator_ata: Option<&Account<'info, TokenAccount>>,
    token_program: &Program<'info, Token>,
    sys_program: &Program<'info, System>,
    amount: u64,
    safety_deposit: u64,
    rescue_start: u32,
    now: u32,
) -> Result<()> {
    require!(
        rescue_start >= now + RESCUE_DELAY,
        EscrowError::InvalidRescueStart
    );

    // TODO: Verify that safety_deposit is enough to cover public_withdraw and public_cancel methods
    if amount == 0 || safety_deposit == 0 {
        return err!(EscrowError::ZeroAmountOrDeposit);
    }

    // Verify that safety_deposit is less than escrow rent_exempt_reserve
    let rent_exempt_reserve = Rent::get()?.minimum_balance(escrow_size);
    if safety_deposit > rent_exempt_reserve {
        return err!(EscrowError::SafetyDepositTooLarge);
    }

    // Check if token is native (WSOL) and is expected to be wrapped
    if escrow_ata.mint == NATIVE_MINT && creator_ata.is_none() {
        // Transfer native tokens from creator to escrow_ata and wrap
        system_program::transfer(
            CpiContext::new(
                sys_program.to_account_info(),
                system_program::Transfer {
                    from: creator.to_account_info(),
                    to: escrow_ata.to_account_info(),
                },
            ),
            amount,
        )?;

        anchor_spl::token::sync_native(CpiContext::new(
            token_program.to_account_info(),
            anchor_spl::token::SyncNative {
                account: escrow_ata.to_account_info(),
            },
        ))?;
    } else {
        // Do SPL token transfer
        anchor_spl::token::transfer(
            CpiContext::new(
                token_program.to_account_info(),
                anchor_spl::token::Transfer {
                    from: creator_ata
                        .ok_or(EscrowError::MissingCreatorAta)?
                        .to_account_info(),
                    to: escrow_ata.to_account_info(),
                    authority: creator.to_account_info(),
                },
            ),
            amount,
        )?;
    }

    Ok(())
}

pub fn withdraw<'info, T>(
    escrow: &Account<'info, T>,
    escrow_bump: u8,
    escrow_ata: &Account<'info, TokenAccount>,
    recipient: &AccountInfo<'info>,
    recipient_ata: Option<&Account<'info, TokenAccount>>,
    token_program: &Program<'info, Token>,
    rent_recipient: &AccountInfo<'info>,
    safety_deposit_recipient: &AccountInfo<'info>,
    secret: [u8; 32],
) -> Result<()>
where
    T: EscrowBase + AccountSerialize + AccountDeserialize + Clone,
{
    // Verify that the secret matches the hashlock
    let hash = hash(&secret).to_bytes();
    if hash != *escrow.hashlock() {
        return err!(EscrowError::InvalidSecret);
    }

    let amount_bytes = escrow.amount().to_be_bytes();
    let safety_deposit_bytes = escrow.safety_deposit().to_be_bytes();
    let rescue_start_bytes = escrow.rescue_start().to_be_bytes();

    let seeds = [
        "escrow".as_bytes(),
        escrow.order_hash(),
        escrow.hashlock(),
        escrow.creator().as_ref(),
        escrow.recipient().as_ref(),
        escrow.token().as_ref(),
        amount_bytes.as_ref(),
        safety_deposit_bytes.as_ref(),
        rescue_start_bytes.as_ref(),
        &[escrow_bump],
    ];

    if escrow_ata.mint != NATIVE_MINT {
        // Transfer tokens from escrow to recipient
        anchor_spl::token::transfer(
            CpiContext::new_with_signer(
                token_program.to_account_info(),
                anchor_spl::token::Transfer {
                    from: escrow_ata.to_account_info(),
                    to: recipient_ata
                        .ok_or(EscrowError::MissingRecipientAta)?
                        .to_account_info(),
                    authority: escrow.to_account_info(),
                },
                &[&seeds],
            ),
            escrow.amount(),
        )?;

        // Close the escrow_ata account
        anchor_spl::token::close_account(CpiContext::new_with_signer(
            token_program.to_account_info(),
            anchor_spl::token::CloseAccount {
                account: escrow_ata.to_account_info(),
                destination: rent_recipient.to_account_info(),
                authority: escrow.to_account_info(),
            },
            &[&seeds],
        ))?;
    } else {
        // Handle native token (WSOL) withdrawal and ata closure
        close_and_withdraw_native_ata(
            escrow,
            escrow_ata,
            recipient,
            recipient_ata,
            token_program,
            seeds,
        )?;
    }
    // Close the escrow account
    close_escrow_account(escrow, safety_deposit_recipient, rent_recipient)?;

    Ok(())
}

pub fn cancel<'info, T>(
    escrow: &Account<'info, T>,
    escrow_bump: u8,
    escrow_ata: &Account<'info, TokenAccount>,
    creator: &AccountInfo<'info>,
    creator_ata: Option<&Account<'info, TokenAccount>>,
    token_program: &Program<'info, Token>,
    rent_recipient: &AccountInfo<'info>,
    safety_deposit_recipient: &AccountInfo<'info>,
) -> Result<()>
where
    T: EscrowBase + AccountSerialize + AccountDeserialize + Clone,
{
    let amount_bytes = escrow.amount().to_be_bytes();
    let safety_deposit_bytes = escrow.safety_deposit().to_be_bytes();
    let rescue_start_bytes = escrow.rescue_start().to_be_bytes();

    let seeds = [
        "escrow".as_bytes(),
        escrow.order_hash(),
        escrow.hashlock(),
        escrow.creator().as_ref(),
        escrow.recipient().as_ref(),
        escrow.token().as_ref(),
        amount_bytes.as_ref(),
        safety_deposit_bytes.as_ref(),
        rescue_start_bytes.as_ref(),
        &[escrow_bump],
    ];

    if escrow_ata.mint != NATIVE_MINT {
        // Return tokens to creator
        anchor_spl::token::transfer(
            CpiContext::new_with_signer(
                token_program.to_account_info(),
                anchor_spl::token::Transfer {
                    from: escrow_ata.to_account_info(),
                    to: creator_ata
                        .ok_or(EscrowError::MissingCreatorAta)?
                        .to_account_info(),
                    authority: escrow.to_account_info(),
                },
                &[&seeds],
            ),
            escrow.amount(),
        )?;

        // Close the escrow_ata account
        anchor_spl::token::close_account(CpiContext::new_with_signer(
            token_program.to_account_info(),
            anchor_spl::token::CloseAccount {
                account: escrow_ata.to_account_info(),
                destination: rent_recipient.to_account_info(),
                authority: escrow.to_account_info(),
            },
            &[&seeds],
        ))?;
    } else {
        // Handle native token (WSOL) withdrawal and ata closure
        close_and_withdraw_native_ata(
            escrow,
            escrow_ata,
            creator,
            creator_ata,
            token_program,
            seeds,
        )?;
    }
    // Close the escrow account
    close_escrow_account(escrow, safety_deposit_recipient, rent_recipient)?;

    Ok(())
}

fn close_escrow_account<'info, T>(
    escrow: &Account<'info, T>,
    safety_deposit_recipient: &AccountInfo<'info>,
    rent_recipient: &AccountInfo<'info>,
) -> Result<()>
where
    T: EscrowBase + AccountSerialize + AccountDeserialize + Clone,
{
    // Transfer safety_deposit from escrow to safety_deposit_recipient
    if rent_recipient.key() != safety_deposit_recipient.key() {
        let safety_deposit = escrow.safety_deposit();
        escrow.sub_lamports(safety_deposit)?;
        safety_deposit_recipient.add_lamports(safety_deposit)?;
    }

    // Close escrow account and transfer remaining lamports to rent_recipient
    escrow.close(rent_recipient.to_account_info())?;

    Ok(())
}

fn close_and_withdraw_native_ata<'info, T>(
    escrow: &Account<'info, T>,
    escrow_ata: &Account<'info, TokenAccount>,
    recipient: &AccountInfo<'info>,
    sol_destination_ata: Option<&Account<'info, TokenAccount>>,
    token_program: &Program<'info, Token>,
    seeds: [&[u8]; 10],
) -> Result<()>
where
    T: EscrowBase + AccountSerialize + AccountDeserialize + Clone,
{
    if sol_destination_ata.is_some() {
        // in case of sol_desination_ata provided, we transfer wSOL from the escrow_ata to sol_destination_ata (without unwrapping)
        anchor_spl::token::transfer(
            CpiContext::new_with_signer(
                token_program.to_account_info(),
                anchor_spl::token::Transfer {
                    from: escrow_ata.to_account_info(),
                    to: sol_destination_ata
                        .ok_or(EscrowError::MissingSolDestination)?
                        .to_account_info(),
                    authority: escrow.to_account_info(),
                },
                &[&seeds],
            ),
            escrow.amount(),
        )?;
    }

    // using escrow pda as an intermediate account to transfer native tokens
    // in case of sol_destination_ata provided, escrow pda will only receive the escrow ata rent-exempt lamports
    // which rent_recipient will receive after closing the escrow
    anchor_spl::token::close_account(CpiContext::new_with_signer(
        token_program.to_account_info(),
        anchor_spl::token::CloseAccount {
            account: escrow_ata.to_account_info(),
            destination: escrow.to_account_info(),
            authority: escrow.to_account_info(),
        },
        &[&seeds],
    ))?;

    if sol_destination_ata.is_none() {
        // Transfer the native tokens from escrow pda to recipient
        escrow.sub_lamports(escrow.amount())?;
        recipient.add_lamports(escrow.amount())?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn rescue_funds<'info>(
    escrow: &AccountInfo<'info>,
    order_hash: [u8; 32],
    hashlock: [u8; 32],
    escrow_creator: Pubkey,
    escrow_mint: Pubkey,
    escrow_amount: u64,
    safety_deposit: u64,
    rescue_start: u32,
    escrow_bump: u8,
    escrow_ata: &Account<'info, TokenAccount>,
    recipient: &AccountInfo<'info>,
    recipient_ata: &Account<'info, TokenAccount>,
    token_program: &Program<'info, Token>,
    rescue_amount: u64,
) -> Result<()> {
    let now = utils::get_current_timestamp()?;
    require!(now >= rescue_start, EscrowError::InvalidTime);

    let amount_bytes = escrow_amount.to_be_bytes();
    let safety_deposit_bytes = safety_deposit.to_be_bytes();
    let rescue_start_bytes = rescue_start.to_be_bytes();

    let recipient_pubkey = recipient.key();

    let seeds = [
        "escrow".as_bytes(),
        order_hash.as_ref(),
        hashlock.as_ref(),
        escrow_creator.as_ref(),
        recipient_pubkey.as_ref(),
        escrow_mint.as_ref(),
        amount_bytes.as_ref(),
        safety_deposit_bytes.as_ref(),
        rescue_start_bytes.as_ref(),
        &[escrow_bump],
    ];

    // Transfer tokens from escrow to recipient
    anchor_spl::token::transfer(
        CpiContext::new_with_signer(
            token_program.to_account_info(),
            anchor_spl::token::Transfer {
                from: escrow_ata.to_account_info(),
                to: recipient_ata.to_account_info(),
                authority: escrow.to_account_info(),
            },
            &[&seeds],
        ),
        rescue_amount,
    )?;

    if rescue_amount == escrow_ata.amount {
        // Close the escrow_ata account
        anchor_spl::token::close_account(CpiContext::new_with_signer(
            token_program.to_account_info(),
            anchor_spl::token::CloseAccount {
                account: escrow_ata.to_account_info(),
                destination: recipient.to_account_info(),
                authority: escrow.to_account_info(),
            },
            &[&seeds],
        ))?;
    }

    Ok(())
}
