use anchor_lang::prelude::*;
use anchor_lang::system_program;

use anchor_spl::token_interface::{
    close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
    TransferChecked,
};

use crate::error::EscrowError;
use crate::utils;

pub fn rescue_funds<'info>(
    escrow: &AccountInfo<'info>,
    rescue_start: Option<u32>,
    escrow_ata: &InterfaceAccount<'info, TokenAccount>,
    recipient: &AccountInfo<'info>,
    recipient_ata: &InterfaceAccount<'info, TokenAccount>,
    mint: &InterfaceAccount<'info, Mint>,
    token_program: &Interface<'info, TokenInterface>,
    rescue_amount: u64,
    seeds: &[&[u8]],
) -> Result<()> {
    let now = utils::get_current_timestamp()?;
    if rescue_start.is_some() {
        require!(
            now >= rescue_start.unwrap(),
            EscrowError::InvalidRescueStart
        );
    }

    // Transfer tokens from escrow to recipient
    uni_transfer(
        &UniTransferParams::TokenTransfer {
            from: escrow_ata.to_account_info(),
            to: recipient_ata.to_account_info(),
            authority: escrow.to_account_info(),
            mint: mint.clone(),
            amount: rescue_amount,
            program: token_program.clone(),
        },
        Some(&[seeds]),
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
            &[seeds],
        ))?;
    }

    Ok(())
}

pub enum UniTransferParams<'info> {
    NativeTransfer {
        from: AccountInfo<'info>,
        to: AccountInfo<'info>,
        amount: u64,
        program: Program<'info, System>,
    },

    TokenTransfer {
        from: AccountInfo<'info>,
        authority: AccountInfo<'info>,
        to: AccountInfo<'info>,
        mint: InterfaceAccount<'info, Mint>,
        amount: u64,
        program: Interface<'info, TokenInterface>,
    },
}

pub fn uni_transfer(
    params: &UniTransferParams<'_>,
    signer_seeds: Option<&[&[&[u8]]]>,
) -> Result<()> {
    match params {
        UniTransferParams::NativeTransfer {
            from,
            to,
            amount,
            program,
        } => {
            if *amount == 0 {
                return Ok(());
            }
            let ctx = system_program::Transfer {
                from: from.to_account_info(),
                to: to.to_account_info(),
            };

            let cpi_ctx = match signer_seeds {
                Some(seeds) => CpiContext::new_with_signer(program.to_account_info(), ctx, seeds),
                None => CpiContext::new(program.to_account_info(), ctx),
            };

            system_program::transfer(cpi_ctx, *amount)
        }

        UniTransferParams::TokenTransfer {
            from,
            authority,
            to,
            mint,
            amount,
            program,
        } => {
            if *amount == 0 {
                return Ok(());
            }
            let ctx = TransferChecked {
                from: from.to_account_info(),
                mint: mint.to_account_info(),
                to: to.to_account_info(),
                authority: authority.to_account_info(),
            };

            let cpi_ctx = match signer_seeds {
                Some(seeds) => CpiContext::new_with_signer(program.to_account_info(), ctx, seeds),
                None => CpiContext::new(program.to_account_info(), ctx),
            };

            transfer_checked(cpi_ctx, *amount, mint.decimals)
        }
    }
}

/// A fee taken from the escrowed amount during a payout. Mirrors the protocol and integrator
/// fees stored in the `parameters` of the EVM escrow immutables.
pub struct FeePayout<'a, 'info> {
    pub amount: u64,
    /// Wallet that receives the fee in lamports if the asset is native.
    pub recipient: Option<&'a AccountInfo<'info>>,
    /// Token account that receives the fee if the asset is not native.
    pub recipient_ata: Option<&'a InterfaceAccount<'info, TokenAccount>>,
}

impl FeePayout<'_, '_> {
    pub fn total(fees: &[Self]) -> u64 {
        fees.iter().map(|fee| fee.amount).sum()
    }
}

pub fn withdraw_and_close_token_ata<'info>(
    escrow_ata: &InterfaceAccount<'info, TokenAccount>,
    authority: &AccountInfo<'info>,
    to: &AccountInfo<'info>,
    mint: &InterfaceAccount<'info, Mint>,
    token_program: &Interface<'info, TokenInterface>,
    rent_recipient: &AccountInfo<'info>,
    amount: u64,
    seeds: &[&[u8]],
) -> Result<()> {
    // Transfer tokens
    uni_transfer(
        &UniTransferParams::TokenTransfer {
            from: escrow_ata.to_account_info(),
            authority: authority.clone(),
            to: to.to_account_info(),
            mint: mint.clone(),
            amount,
            program: token_program.clone(),
        },
        Some(&[seeds]),
    )?;

    // Close the escrow_ata account
    close_account(CpiContext::new_with_signer(
        token_program.to_account_info(),
        CloseAccount {
            account: escrow_ata.to_account_info(),
            destination: rent_recipient.to_account_info(),
            authority: authority.clone(),
        },
        &[seeds],
    ))?;
    Ok(())
}

/// Pays out the escrowed asset: the fees go to their recipients, the rest goes to `recipient`.
/// With an empty `fees` slice the whole amount goes to `recipient`.
#[allow(clippy::too_many_arguments)]
pub fn process_payout<'info>(
    mint: &InterfaceAccount<'info, Mint>,
    asset_is_native: bool,
    escrow_amount: u64,
    escrow: &AccountInfo<'info>,
    escrow_ata: &InterfaceAccount<'info, TokenAccount>,
    recipient: &AccountInfo<'info>,
    recipient_ata: Option<&InterfaceAccount<'info, TokenAccount>>,
    rent_recipient: &AccountInfo<'info>,
    seeds: [&[u8]; 6],
    token_program: &Interface<'info, TokenInterface>,
    fees: &[FeePayout<'_, 'info>],
) -> Result<()> {
    let total_fees = FeePayout::total(fees);

    if asset_is_native {
        // Using escrow pda as an intermediate account to transfer native tokens
        // the leftover lamports from ata's rent will be transferred to the rent recipient
        // after closing the escrow account
        close_account(CpiContext::new_with_signer(
            token_program.to_account_info(),
            CloseAccount {
                account: escrow_ata.to_account_info(),
                destination: escrow.to_account_info(),
                authority: escrow.to_account_info(),
            },
            &[&seeds],
        ))?;

        // Transfer the fees from escrow pda to the fee recipients
        for fee in fees.iter().filter(|fee| fee.amount > 0) {
            let fee_recipient = fee.recipient.ok_or(EscrowError::MissingFeeRecipient)?;
            escrow.sub_lamports(fee.amount)?;
            fee_recipient.add_lamports(fee.amount)?;
        }

        // Transfer the native tokens from escrow pda to recipient
        escrow.sub_lamports(escrow_amount - total_fees)?;
        recipient.add_lamports(escrow_amount - total_fees)?;
    } else {
        // Transfer the fees from escrow ata to the fee recipients
        for fee in fees.iter().filter(|fee| fee.amount > 0) {
            let fee_recipient_ata = fee
                .recipient_ata
                .ok_or(EscrowError::MissingFeeRecipientAta)?;
            uni_transfer(
                &UniTransferParams::TokenTransfer {
                    from: escrow_ata.to_account_info(),
                    authority: escrow.to_account_info(),
                    to: fee_recipient_ata.to_account_info(),
                    mint: mint.clone(),
                    amount: fee.amount,
                    program: token_program.clone(),
                },
                Some(&[&seeds]),
            )?;
        }

        // Transfer the rest of the escrow ata balance to the recipient and close the ata
        withdraw_and_close_token_ata(
            escrow_ata,
            &escrow.to_account_info(),
            &recipient_ata
                .ok_or(EscrowError::MissingRecipientAta)?
                .to_account_info(),
            mint,
            token_program,
            rent_recipient,
            escrow_ata.amount - total_fees,
            &seeds,
        )?;
    }

    Ok(())
}
