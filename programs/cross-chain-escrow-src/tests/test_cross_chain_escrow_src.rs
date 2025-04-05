use anchor_lang::prelude::AccountInfo;
use common::error::EscrowError;
use common_tests::helpers::src_program::SrcProgram;
use common_tests::helpers::*;
use common_tests::tests as common_escrow_tests;
use common_tests::wrap_entry;

use anchor_lang::{InstructionData, Space};
use anchor_spl::{
    associated_token::ID as spl_associated_token_id, token::spl_token::ID as spl_program_id,
};
use solana_program::{
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey::Pubkey,
    system_program::ID as system_program_id,
    sysvar::rent::ID as rent_id,
};
use solana_program_runtime::invoke_context::BuiltinFunctionWithContext;
use solana_program_test::{processor, tokio};
use solana_sdk::{signature::Signer, transaction::Transaction};
use test_context::test_context;

type TestState = TestStateBase<SrcProgram>;

mod test_escrow_creation {
    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_with_zero_amount(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation_fail_with_zero_amount(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_with_zero_safety_deposit(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation_fail_with_zero_safety_deposit(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_with_insufficient_safety_deposit(
        test_state: &mut TestState,
    ) {
        common_escrow_tests::test_escrow_creation_fail_with_insufficient_safety_deposit(test_state)
            .await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_with_insufficient_tokens(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation_fail_with_insufficient_tokens(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_with_existing_order_hash(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation_fail_with_existing_order_hash(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_if_finality_duration_overflows(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation_fail_if_finality_duration_overflows(test_state)
            .await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_if_withdrawal_duration_overflows(
        test_state: &mut TestState,
    ) {
        common_escrow_tests::test_escrow_creation_fail_if_withdrawal_duration_overflows(test_state)
            .await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_if_public_withdrawal_duration_overflows(
        test_state: &mut TestState,
    ) {
        common_escrow_tests::test_escrow_creation_fail_if_public_withdrawal_duration_overflows(
            test_state,
        )
        .await
    }

    #[test_context(TestState)]
    #[tokio::test]
    pub async fn test_escrow_creation_fail_if_cancellation_duration_overflows(
        test_state: &mut TestState,
    ) {
        test_state.test_arguments.cancellation_duration = u32::MAX;
        let (_, _, tx_result) = create_escrow_tx(test_state).await;
        tx_result.expect_error((0, ProgramError::ArithmeticOverflow));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_with_invalid_rescue_start(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation_fail_with_invalid_rescue_start(test_state).await
    }
}

mod test_escrow_withdraw {
    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    pub async fn test_withdraw(test_state: &mut TestStateBase<SrcProgram>) {
        common_escrow_tests::test_withdraw(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    pub async fn test_withdraw_does_not_work_with_wrong_secret(
        test_state: &mut TestStateBase<SrcProgram>,
    ) {
        common_escrow_tests::test_withdraw_does_not_work_with_wrong_secret(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    pub async fn test_withdraw_does_not_work_with_non_recipient(
        test_state: &mut TestStateBase<SrcProgram>,
    ) {
        common_escrow_tests::test_withdraw_does_not_work_with_non_recipient(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw_does_not_work_with_wrong_recipient_ata(test_state: &mut TestState) {
        common_escrow_tests::test_withdraw_does_not_work_with_wrong_recipient_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw_does_not_work_with_wrong_escrow_ata(test_state: &mut TestState) {
        common_escrow_tests::test_withdraw_does_not_work_with_wrong_escrow_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw_does_not_work_before_withdrawal_start(test_state: &mut TestState) {
        common_escrow_tests::test_withdraw_does_not_work_before_withdrawal_start(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw_does_not_work_after_cancellation_start(test_state: &mut TestState) {
        common_escrow_tests::test_withdraw_does_not_work_after_cancellation_start(test_state).await
    }
}

mod test_escrow_public_withdraw {
    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_fails_before_start_of_public_withdraw(
        test_state: &mut TestState,
    ) {
        common_escrow_tests::test_public_withdraw_fails_before_start_of_public_withdraw(test_state)
            .await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_fails_after_cancellation_start(test_state: &mut TestState) {
        common_escrow_tests::test_public_withdraw_fails_after_cancellation_start(test_state).await
    }
}
mod test_escrow_cancel {
    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel(test_state: &mut TestState) {
        common_escrow_tests::test_cancel(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cannot_cancel_by_non_creator(test_state: &mut TestState) {
        common_escrow_tests::test_cannot_cancel_by_non_creator(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cannot_cancel_with_wrong_creator_ata(test_state: &mut TestState) {
        common_escrow_tests::test_cannot_cancel_with_wrong_creator_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cannot_cancel_with_wrong_escrow_ata(test_state: &mut TestState) {
        common_escrow_tests::test_cannot_cancel_with_wrong_escrow_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cannot_cancel_before_cancellation_start(test_state: &mut TestState) {
        common_escrow_tests::test_cannot_cancel_before_cancellation_start(test_state).await
    }
}

mod test_escrow_public_cancel {
    use super::*;
    use local_helpers::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cannot_public_cancel_before_public_cancellation_start(
        test_state: &mut TestState,
    ) {
        let (escrow, escrow_ata) = create_escrow(test_state).await;
        let public_cancel_ix = create_public_cancel_ix(test_state, &escrow, &escrow_ata);

        let transaction = Transaction::new_signed_with_payer(
            &[public_cancel_ix],
            Some(&test_state.payer_kp.pubkey()),
            &[&test_state.payer_kp],
            test_state.context.last_blockhash,
        );

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
        );
        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((0, ProgramError::Custom(EscrowError::InvalidTime.into())))
    }
}

mod test_escrow_rescue_funds {
    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_all_tokens_and_close_ata(test_state: &mut TestState) {
        common_escrow_tests::test_rescue_all_tokens_and_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_part_of_tokens_and_not_close_ata(test_state: &mut TestState) {
        common_escrow_tests::test_rescue_part_of_tokens_and_not_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cannot_rescue_funds_before_rescue_delay_pass(test_state: &mut TestState) {
        common_escrow_tests::test_cannot_rescue_funds_before_rescue_delay_pass(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cannot_rescue_funds_by_non_recipient(test_state: &mut TestState) {
        common_escrow_tests::test_cannot_rescue_funds_by_non_recipient(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cannot_rescue_funds_with_wrong_recipient_ata(test_state: &mut TestState) {
        common_escrow_tests::test_cannot_rescue_funds_with_wrong_recipient_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cannot_rescue_funds_with_wrong_escrow_ata(test_state: &mut TestState) {
        common_escrow_tests::test_cannot_rescue_funds_with_wrong_escrow_ata(test_state).await
    }
}

mod local_helpers {
    use super::*;

    use anchor_lang::InstructionData;
    use anchor_spl::token::spl_token::ID as spl_program_id;
    use solana_program::instruction::{AccountMeta, Instruction};
    use solana_program::pubkey::Pubkey;
    use solana_program::system_program::ID as system_program_id;
    use solana_sdk::signature::Signer;

    pub fn create_public_cancel_ix(
        test_state: &TestState,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Instruction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_src::instruction::PublicCancel {});

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_src::id(),
            accounts: vec![
                AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), false),
                AccountMeta::new_readonly(test_state.token, false),
                AccountMeta::new_readonly(test_state.context.payer.pubkey(), true),
                AccountMeta::new(*escrow, false),
                AccountMeta::new(*escrow_ata, false),
                AccountMeta::new(test_state.creator_wallet.token_account, false),
                AccountMeta::new_readonly(spl_program_id, false),
                AccountMeta::new_readonly(system_program_id, false),
            ],
            data: instruction_data,
        };

        instruction
    }
}
