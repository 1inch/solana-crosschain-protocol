use crate::constants::RESCUE_DELAY;
use crate::error::EscrowError;
use crate::utils;
use anchor_lang::prelude::*;
use anchor_lang::solana_program::keccak::hash;
use anchor_spl::token_interface::{
    close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
    TransferChecked,
};

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
    escrow_ata: &InterfaceAccount<'info, TokenAccount>,
    creator_ata: &InterfaceAccount<'info, TokenAccount>,
    mint: &InterfaceAccount<'info, Mint>,
    token_program: &Interface<'info, TokenInterface>,
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

    // Transfer tokens from creator to escrow
    transfer_checked(
        CpiContext::new(
            token_program.to_account_info(),
            TransferChecked {
                from: creator_ata.to_account_info(),
                to: escrow_ata.to_account_info(),
                authority: creator.to_account_info(),
                mint: mint.to_account_info(),
            },
        ),
        amount,
        mint.decimals,
    )?;
    Ok(())
}

pub fn withdraw<'info, T>(
    escrow: &Account<'info, T>,
    escrow_bump: u8,
    escrow_ata: &InterfaceAccount<'info, TokenAccount>,
    recipient_ata: &InterfaceAccount<'info, TokenAccount>,
    mint: &InterfaceAccount<'info, Mint>,
    token_program: &Interface<'info, TokenInterface>,
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

    // Transfer tokens from escrow to recipient
    transfer_checked(
        CpiContext::new_with_signer(
            token_program.to_account_info(),
            TransferChecked {
                from: escrow_ata.to_account_info(),
                to: recipient_ata.to_account_info(),
                authority: escrow.to_account_info(),
                mint: mint.to_account_info(),
            },
            &[&seeds],
        ),
        escrow.amount(),
        mint.decimals,
    )?;

    // Close the escrow_ata account
    close_account(CpiContext::new_with_signer(
        token_program.to_account_info(),
        CloseAccount {
            account: escrow_ata.to_account_info(),
            destination: rent_recipient.to_account_info(),
            authority: escrow.to_account_info(),
        },
        &[&seeds],
    ))?;

    // Close the escrow account
    close_escrow_account(escrow, safety_deposit_recipient, rent_recipient)?;

    Ok(())
}

pub fn cancel<'info, T>(
    escrow: &Account<'info, T>,
    escrow_bump: u8,
    escrow_ata: &InterfaceAccount<'info, TokenAccount>,
    creator_ata: &InterfaceAccount<'info, TokenAccount>,
    mint: &InterfaceAccount<'info, Mint>,
    token_program: &Interface<'info, TokenInterface>,
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

    // Return tokens to creator
    transfer_checked(
        CpiContext::new_with_signer(
            token_program.to_account_info(),
            TransferChecked {
                from: escrow_ata.to_account_info(),
                to: creator_ata.to_account_info(),
                authority: escrow.to_account_info(),
                mint: mint.to_account_info(),
            },
            &[&seeds],
        ),
        escrow.amount(),
        mint.decimals,
    )?;

    // Close the escrow_ata account
    close_account(CpiContext::new_with_signer(
        token_program.to_account_info(),
        CloseAccount {
            account: escrow_ata.to_account_info(),
            destination: creator.to_account_info(),
            authority: escrow.to_account_info(),
        },
        &[&seeds],
    ))?;

    // Close the escrow account
    close_escrow_account(escrow, safety_deposit_recipient, creator)?;

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
    escrow_ata: &InterfaceAccount<'info, TokenAccount>,
    recipient: &AccountInfo<'info>,
    recipient_ata: &InterfaceAccount<'info, TokenAccount>,
    mint: &InterfaceAccount<'info, Mint>,
    token_program: &Interface<'info, TokenInterface>,
    rescue_amount: u64,
) -> Result<()> {
    let now = utils::get_current_timestamp()?;
    require!(now >= rescue_start, EscrowError::InvalidTime);

    let recipient_pubkey = recipient.key();

    let seeds = [
        "escrow".as_bytes(),
        order_hash.as_ref(),
        hashlock.as_ref(),
        escrow_creator.as_ref(),
        recipient_pubkey.as_ref(),
        escrow_mint.as_ref(),
        &escrow_amount.to_be_bytes(),
        &safety_deposit.to_be_bytes(),
        &rescue_start.to_be_bytes(),
        &[escrow_bump],
    ];

    // Transfer tokens from escrow to recipient
    transfer_checked(
        CpiContext::new_with_signer(
            token_program.to_account_info(),
            TransferChecked {
                from: escrow_ata.to_account_info(),
                to: recipient_ata.to_account_info(),
                authority: escrow.to_account_info(),
                mint: mint.to_account_info(),
            },
            &[&seeds],
        ),
        rescue_amount,
        mint.decimals,
    )?;

    if rescue_amount == escrow_ata.amount {
        // Close the escrow_ata account
        close_account(CpiContext::new_with_signer(
            token_program.to_account_info(),
            CloseAccount {
                account: escrow_ata.to_account_info(),
                destination: recipient.to_account_info(),
                authority: escrow.to_account_info(),
            },
            &[&seeds],
        ))?;
    }

    Ok(())
}
