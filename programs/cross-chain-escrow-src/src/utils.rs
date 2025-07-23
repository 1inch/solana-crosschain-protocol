use anchor_lang::{prelude::*, solana_program::keccak::hash};
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};
use common::{
    error::EscrowError,
    escrow::{process_payout, withdraw_and_close_token_ata},
};

use crate::EscrowSrc;

pub fn withdraw<'info>(
    escrow: &Account<'info, EscrowSrc>,
    escrow_bump: u8,
    escrow_ata: &InterfaceAccount<'info, TokenAccount>,
    taker_ata: &InterfaceAccount<'info, TokenAccount>,
    mint: &InterfaceAccount<'info, Mint>,
    token_program: &Interface<'info, TokenInterface>,
    rent_recipient: &AccountInfo<'info>,
    safety_deposit_recipient: &AccountInfo<'info>,
    secret: [u8; 32],
) -> Result<()> {
    // Verify that the secret matches the hashlock
    require!(
        hash(&secret).to_bytes() == escrow.hashlock,
        EscrowError::InvalidSecret
    );

    let seeds = [
        "escrow".as_bytes(),
        &escrow.order_hash,
        &escrow.hashlock,
        escrow.taker.as_ref(),
        &escrow.amount.to_be_bytes(),
        &[escrow_bump],
    ];

    withdraw_and_close_token_ata(
        escrow_ata,
        &escrow.to_account_info(),
        &taker_ata.to_account_info(),
        mint,
        token_program,
        rent_recipient,
        &seeds,
    )?;

    // Disrtibute the safety deposit if needed
    if rent_recipient.key() != safety_deposit_recipient.key() {
        escrow.sub_lamports(escrow.safety_deposit)?;
        safety_deposit_recipient.add_lamports(escrow.safety_deposit)?;
    }

    Ok(())
}

pub fn cancel<'info>(
    escrow: &Account<'info, EscrowSrc>,
    escrow_bump: u8,
    escrow_ata: &InterfaceAccount<'info, TokenAccount>,
    creator_ata: Option<&InterfaceAccount<'info, TokenAccount>>,
    mint: &InterfaceAccount<'info, Mint>,
    token_program: &Interface<'info, TokenInterface>,
    rent_recipient: &AccountInfo<'info>,
    creator: &AccountInfo<'info>,
    safety_deposit_recipient: &AccountInfo<'info>,
) -> Result<()> {
    let seeds = [
        "escrow".as_bytes(),
        &escrow.order_hash,
        &escrow.hashlock,
        escrow.taker.as_ref(),
        &escrow.amount.to_be_bytes(),
        &[escrow_bump],
    ];

    process_payout(
        mint,
        escrow.asset_is_native,
        escrow.amount,
        &escrow.to_account_info(),
        escrow_ata,
        creator,
        creator_ata,
        rent_recipient,
        seeds,
        token_program,
    )?;

    // Disrtibute the safety deposit if needed
    if rent_recipient.key() != safety_deposit_recipient.key() {
        escrow.sub_lamports(escrow.safety_deposit)?;
        safety_deposit_recipient.add_lamports(escrow.safety_deposit)?;
    }

    Ok(())
}
