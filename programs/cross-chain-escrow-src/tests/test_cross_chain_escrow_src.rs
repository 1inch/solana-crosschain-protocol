use anchor_lang::prelude::ProgramError;
use common::error::EscrowError;
use common_tests::helpers::*;
use common_tests::run_for_tokens;
use common_tests::src_program::SrcProgram;
use common_tests::tests as common_escrow_tests;
use solana_program_test::tokio;
use solana_sdk::signature::Signer;
use solana_sdk::signer::keypair::Keypair;
use test_context::test_context;

run_for_tokens!(
    (Token2020, token_2020_tests),
    (Token2022, token_2022_tests) | SrcProgram,
    mod token_module {

        use super::*;

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
            async fn test_escrow_creation_fail_with_insufficient_funds(test_state: &mut TestState) {
                common_escrow_tests::test_escrow_creation_fail_with_insufficient_funds(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fail_with_insufficient_tokens(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_escrow_creation_fail_with_insufficient_tokens(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fail_with_existing_order_hash(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_escrow_creation_fail_with_existing_order_hash(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fail_if_finality_duration_overflows(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_escrow_creation_fail_if_finality_duration_overflows(
                    test_state,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fail_if_withdrawal_duration_overflows(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_escrow_creation_fail_if_withdrawal_duration_overflows(
                    test_state,
                )
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
                let (_, _, transaction) = create_escrow_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((0, ProgramError::ArithmeticOverflow));
            }
        }

        mod test_escrow_withdraw {
            use super::*;
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
            pub async fn test_withdraw_only(test_state: &mut TestState) {
                common_escrow_tests::test_withdraw(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            pub async fn test_withdraw_does_not_work_with_wrong_secret(test_state: &mut TestState) {
                common_escrow_tests::test_withdraw_does_not_work_with_wrong_secret(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            pub async fn test_withdraw_does_not_work_with_non_recipient(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_withdraw_does_not_work_with_non_recipient(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_does_not_work_with_wrong_recipient_ata(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_withdraw_does_not_work_with_wrong_recipient_ata(
                    test_state,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_does_not_work_with_wrong_escrow_ata(test_state: &mut TestState) {
                common_escrow_tests::test_withdraw_does_not_work_with_wrong_escrow_ata(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_does_not_work_before_withdrawal_start(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_withdraw_does_not_work_before_withdrawal_start(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_does_not_work_after_cancellation_start(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_withdraw_does_not_work_after_cancellation_start(
                    test_state,
                )
                .await
            }
        }

        mod test_escrow_public_withdraw {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_tokens_by_recipient(test_state: &mut TestState) {
                common_escrow_tests::test_public_withdraw_tokens(
                    test_state,
                    test_state.recipient_wallet.keypair.insecure_clone(),
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_tokens_by_any_account(test_state: &mut TestState) {
                let withdrawer = Keypair::new();
                transfer_lamports(
                    &mut test_state.context,
                    WALLET_DEFAULT_LAMPORTS,
                    &test_state.payer_kp,
                    &withdrawer.pubkey(),
                )
                .await;
                common_escrow_tests::test_public_withdraw_tokens(test_state, withdrawer).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_with_wrong_secret(test_state: &mut TestState) {
                common_escrow_tests::test_public_withdraw_fails_with_wrong_secret(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_with_wrong_recipient_ata(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_public_withdraw_fails_with_wrong_recipient_ata(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_with_wrong_escrow_ata(test_state: &mut TestState) {
                common_escrow_tests::test_public_withdraw_fails_with_wrong_escrow_ata(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_before_start_of_public_withdraw(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_public_withdraw_fails_before_start_of_public_withdraw(
                    test_state,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_after_cancellation_start(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_public_withdraw_fails_after_cancellation_start(test_state)
                    .await
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
            use super::local_helpers::*;
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_public_cancel_before_public_cancellation_start(
                test_state: &mut TestState,
            ) {
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                let transaction =
                    create_public_cancel_tx(test_state, &escrow, &escrow_ata, &test_state.payer_kp);

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
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
                common_escrow_tests::test_cannot_rescue_funds_before_rescue_delay_pass(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_by_non_recipient(test_state: &mut TestState) {
                common_escrow_tests::test_cannot_rescue_funds_by_non_recipient(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_with_wrong_recipient_ata(test_state: &mut TestState) {
                common_escrow_tests::test_cannot_rescue_funds_with_wrong_recipient_ata(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_with_wrong_escrow_ata(test_state: &mut TestState) {
                common_escrow_tests::test_cannot_rescue_funds_with_wrong_escrow_ata(test_state)
                    .await
            }
        }
    }
);

mod test_escrow_creation_cost {
    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_tx_cost(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation_tx_cost(test_state).await
    }
}

mod local_helpers {
    use super::*;

    use anchor_lang::InstructionData;
    use solana_program::instruction::{AccountMeta, Instruction};
    use solana_program::pubkey::Pubkey;
    use solana_program::system_program::ID as system_program_id;
    use solana_sdk::signature::Signer;
    use solana_sdk::transaction::Transaction;

    pub fn create_public_cancel_tx<S: TokenVariant>(
        test_state: &TestStateBase<SrcProgram, S>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
        canceller: &Keypair,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_src::instruction::PublicCancel {});

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_src::id(),
            accounts: vec![
                AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), false),
                AccountMeta::new_readonly(test_state.token, false),
                AccountMeta::new(canceller.pubkey(), true),
                AccountMeta::new(*escrow, false),
                AccountMeta::new(*escrow_ata, false),
                AccountMeta::new(test_state.creator_wallet.token_account, false),
                AccountMeta::new_readonly(S::get_token_program_id(), false),
                AccountMeta::new_readonly(system_program_id, false),
            ],
            data: instruction_data,
        };

        Transaction::new_signed_with_payer(
            &[instruction],
            Some(&test_state.payer_kp.pubkey()),
            &[&test_state.payer_kp, canceller],
            test_state.context.last_blockhash,
        )
    }
}
