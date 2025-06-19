use anchor_lang::{prelude::*, solana_program::keccak::hash};
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};
use common::error::EscrowError;
use common::escrow::EscrowBase;

pub fn withdraw<'info, T>(
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
    // Verify that the secret matches the hashlock
    require!(
        hash(&secret).to_bytes() == *escrow.hashlock(),
        EscrowError::InvalidSecret
    );

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

    if escrow.asset_is_native() {
        common::escrow::close_and_withdraw_native_ata(
            escrow,
            escrow_ata,
            recipient,
            token_program,
            seeds,
        )?;
    } else {
        common::escrow::withdraw_and_close_token_ata(
            &escrow_ata.to_account_info(),
            &escrow.to_account_info(),
            &recipient_ata
                .ok_or(EscrowError::MissingRecipientAta)?
                .to_account_info(),
            mint,
            escrow_ata.amount,
            token_program,
            escrow_ata,
            rent_recipient,
            &seeds,
        )?;
    }

    // Close the escrow account
    common::escrow::close_escrow_account(escrow, safety_deposit_recipient, rent_recipient)?;

    Ok(())
}
