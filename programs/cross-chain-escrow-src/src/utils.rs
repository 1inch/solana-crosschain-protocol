use anchor_lang::{prelude::*, solana_program::keccak::hash};
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};
use common::{
    error::EscrowError,
    escrow::{close_and_withdraw_native_ata, withdraw_and_close_token_ata},
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
        escrow.maker.as_ref(),
        escrow.taker.as_ref(),
        escrow.token.as_ref(),
        &escrow.amount.to_be_bytes(),
        &escrow.safety_deposit.to_be_bytes(),
        &[escrow_bump],
    ];

    common::escrow::withdraw_and_close_token_ata(
        &escrow_ata.to_account_info(),
        &escrow.to_account_info(),
        &taker_ata.to_account_info(),
        mint,
        escrow_ata.amount,
        token_program,
        escrow_ata,
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
        escrow.maker.as_ref(),
        escrow.taker.as_ref(),
        escrow.token.as_ref(),
        &escrow.amount.to_be_bytes(),
        &escrow.safety_deposit.to_be_bytes(),
        &[escrow_bump],
    ];

    if escrow.asset_is_native {
        close_and_withdraw_native_ata(
            &escrow.to_account_info(),
            escrow.amount,
            escrow_ata,
            creator,
            token_program,
            seeds,
        )?;
    } else {
        withdraw_and_close_token_ata(
            &escrow_ata.to_account_info(),
            &escrow.to_account_info(),
            &creator_ata
                .ok_or(EscrowError::MissingCreatorAta)?
                .to_account_info(),
            mint,
            escrow_ata.amount,
            token_program,
            escrow_ata,
            rent_recipient,
            &seeds,
        )?;
    }

    // Disrtibute the safety deposit if needed
    if rent_recipient.key() != safety_deposit_recipient.key() {
        escrow.sub_lamports(escrow.safety_deposit)?;
        safety_deposit_recipient.add_lamports(escrow.safety_deposit)?;
    }

    Ok(())
}
