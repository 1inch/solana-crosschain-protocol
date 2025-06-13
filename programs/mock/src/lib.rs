use anchor_lang::prelude::*;
use anchor_spl::*;

declare_id!("FcNMjoERX8zdpkfEzaFw3xxiAoU8iv85reQ8Xj3uEcxf");

#[account]
#[derive(InitSpace)]
pub struct EscrowSrc {
    order_hash: [u8; 32],
    hashlock: [u8; 32],
    maker: Pubkey,
    taker: Pubkey,
    token: Pubkey,
    amount: u64,
    safety_deposit: u64,
    withdrawal_start: u32,
    public_withdrawal_start: u32,
    cancellation_start: u32,
    public_cancellation_start: u32,
    rescue_start: u32,
    asset_is_native: bool,
    dst_amount: [u64; 4],
}

#[program]
pub mod mock {
    use super::*;

    pub fn create_escrow(ctx: Context<CreateEscrow>, amount: u64) -> Result<()> {
        ctx.accounts.escrow.set_inner(EscrowSrc {
            order_hash: [0; 32],
            hashlock: [0; 32],
            maker: ctx.accounts.creator.key(),
            taker: ctx.accounts.taker.key(),
            token: ctx.accounts.mint.key(),
            amount: 20,
            safety_deposit: 20,
            withdrawal_start: 0,
            public_withdrawal_start: 0,
            cancellation_start: 0,
            public_cancellation_start: 9,
            rescue_start: 0,
            asset_is_native: true,
            dst_amount: [0; 4],
        });

        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(amount: u64)]
pub struct CreateEscrow<'info> {
    #[account(mut)]
    mint: AccountInfo<'info>,
    #[account(mut)]
    creator: AccountInfo<'info>,
    #[account(mut)]
    taker: AccountInfo<'info>,
    /// Account to store escrow details
    #[account(
        init,
        payer = taker,
        space = 8 + EscrowSrc::INIT_SPACE,
        seeds = ["order".as_bytes()],
        bump
    )]
    escrow: Box<Account<'info, EscrowSrc>>,
    system_program: Program<'info, System>,
}
