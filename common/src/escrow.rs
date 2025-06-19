use anchor_lang::prelude::*;
use anchor_lang::system_program;

use anchor_spl::token_interface::{
    close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
    TransferChecked,
};

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

    fn asset_is_native(&self) -> bool;

    fn escrow_type(&self) -> EscrowType;
}

pub fn cancel<'info, T>(
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

    if escrow.asset_is_native() {
        close_and_withdraw_native_ata(escrow, escrow_ata, creator, token_program, seeds)?;
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

    // Close the escrow account
    close_escrow_account(escrow, safety_deposit_recipient, rent_recipient)?;

    Ok(())
}

pub fn rescue_funds<'info>(
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
    let now = utils::get_current_timestamp()?;
    require!(now >= rescue_start, EscrowError::InvalidTime);

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

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq)]
pub enum EscrowType {
    Src,
    Dst,
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

// Handle native token transfer or WSOL unwrapping and ata closure
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

    // Transfer the native tokens from escrow pda to recipient
    escrow.sub_lamports(escrow.amount())?;
    recipient.add_lamports(escrow.amount())?;

    Ok(())
}

fn withdraw_and_close_token_ata<'info>(
    from: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    to: &AccountInfo<'info>,
    mint: &InterfaceAccount<'info, Mint>,
    amount: u64,
    token_program: &Interface<'info, TokenInterface>,
    escrow_ata: &InterfaceAccount<'info, TokenAccount>,
    rent_recipient: &AccountInfo<'info>,
    seeds: &[&[u8]],
) -> Result<()> {
    // Transfer tokens
    uni_transfer(
        &UniTransferParams::TokenTransfer {
            from: from.clone(),
            authority: authority.clone(),
            to: to.clone(),
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
