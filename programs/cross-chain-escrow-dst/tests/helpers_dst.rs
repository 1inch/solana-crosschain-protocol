use std::marker::PhantomData;

use common_tests::{
    dst_program::DstProgram,
    helpers::{TestStateBase, TokenVariant},
};
use solana_program::pubkey::Pubkey;
use solana_sdk::signer::Signer;

pub async fn mint_excess_tokens<S: TokenVariant>(
    test_state: &mut TestStateBase<DstProgram, S>,
    escrow_ata: &Pubkey,
    excess_amount: u64,
) {
    S::mint_spl_tokens(
        &mut test_state.context,
        &test_state.token,
        escrow_ata,
        &test_state.payer_kp.pubkey(),
        &test_state.payer_kp,
        excess_amount,
    )
    .await;
}

pub fn get_token_account_len<S: TokenVariant>(
    _: PhantomData<TestStateBase<DstProgram, S>>,
) -> usize {
    S::get_token_account_size()
}
