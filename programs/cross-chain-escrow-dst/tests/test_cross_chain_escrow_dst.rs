use common::error::EscrowError;
use common_tests::dst_program::DstProgram;
use common_tests::helpers::*;
use common_tests::tests as common_escrow_tests;

use solana_program::program_error::ProgramError;
use solana_program_test::tokio;
use solana_sdk::{signature::Signer, sysvar::clock::Clock, transaction::Transaction};
use test_context::test_context;

type TestState = TestStateBase<DstProgram>;

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
    async fn test_escrow_creation_fail_when_cancellation_start_gt_src_cancellation_timestamp(
        test_state: &mut TestState,
    ) {
        let c: Clock = test_state.client.get_sysvar().await.unwrap();
        test_state.test_arguments.src_cancellation_timestamp = c.unix_timestamp as u32 + 1;
        let (_, _, create_ix) = create_escrow_data(test_state);

        let transaction = Transaction::new_signed_with_payer(
            &[create_ix],
            Some(&test_state.payer_kp.pubkey()),
            &[
                &test_state.context.payer,
                &test_state.creator_wallet.keypair,
            ],
            test_state.context.last_blockhash,
        );

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                0,
                ProgramError::Custom(EscrowError::InvalidCreationTime.into()),
            ))
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
    pub async fn test_withdraw(test_state: &mut TestStateBase<DstProgram>) {
        common_escrow_tests::test_withdraw(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    pub async fn test_withdraw_does_not_work_with_wrong_secret(
        test_state: &mut TestStateBase<DstProgram>,
    ) {
        common_escrow_tests::test_withdraw_does_not_work_with_wrong_secret(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    pub async fn test_withdraw_does_not_work_with_non_recipient(
        test_state: &mut TestStateBase<DstProgram>,
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
