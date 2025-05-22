use anchor_lang::prelude::{AccountInfo, ErrorCode};
use anchor_spl::token::spl_token::state::Account as SplTokenAccount;
use common::error::EscrowError;
use common_tests::helpers::*;
use common_tests::tests as common_escrow_tests;
use common_tests::{run_for_tokens, wrap_entry};
use solana_sdk::signer::keypair::Keypair;

use anchor_lang::{InstructionData, Space};
use anchor_spl::associated_token::ID as spl_associated_token_id;
use solana_program::{
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    system_program::ID as system_program_id,
    sysvar::rent::ID as rent_id,
};
use solana_program_runtime::invoke_context::BuiltinFunctionWithContext;
use solana_program_test::{processor, tokio};
use solana_sdk::{signature::Signer, transaction::Transaction};
use test_context::test_context;

struct SrcProgram;

mod traits {
    use super::*;

    type TestState<S> = TestStateBase<SrcProgram, S>;
    impl<S: TokenVariant> EscrowVariant<S> for SrcProgram {
        fn get_program_spec() -> (Pubkey, Option<BuiltinFunctionWithContext>) {
            (
                cross_chain_escrow_src::id(),
                wrap_entry!(cross_chain_escrow_src::entry),
            )
        }

        fn get_public_withdraw_tx(
            test_state: &TestState<S>,
            escrow: &Pubkey,
            escrow_ata: &Pubkey,
            withdrawer: &Keypair,
        ) -> Transaction {
            let instruction_data =
                InstructionData::data(&cross_chain_escrow_src::instruction::PublicWithdraw {
                    secret: test_state.secret,
                });

            let instruction: Instruction = Instruction {
                program_id: cross_chain_escrow_src::id(),
                accounts: vec![
                    AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), false),
                    AccountMeta::new_readonly(test_state.recipient_wallet.keypair.pubkey(), false),
                    AccountMeta::new(withdrawer.pubkey(), true),
                    AccountMeta::new_readonly(test_state.token, false),
                    AccountMeta::new(*escrow, false),
                    AccountMeta::new(*escrow_ata, false),
                    AccountMeta::new(test_state.recipient_wallet.token_account, false),
                    AccountMeta::new_readonly(S::get_token_program_id(), false),
                    AccountMeta::new_readonly(system_program_id, false),
                ],
                data: instruction_data,
            };

            Transaction::new_signed_with_payer(
                &[instruction],
                Some(&test_state.payer_kp.pubkey()), // so that withdrawer does not incurr transaction
                // charges and mess up computation of withdrawer's
                // balance expectation.
                &[withdrawer, &test_state.payer_kp],
                test_state.context.last_blockhash,
            )
        }

        fn get_cancel_tx(
            test_state: &TestState<S>,
            escrow: &Pubkey,
            escrow_ata: &Pubkey,
        ) -> Transaction {
            let instruction_data =
                InstructionData::data(&cross_chain_escrow_src::instruction::Cancel {});

            let instruction: Instruction = Instruction {
                program_id: cross_chain_escrow_src::id(),
                accounts: vec![
                    AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), true),
                    AccountMeta::new_readonly(test_state.token, false),
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
                &[
                    &test_state.context.payer,
                    &test_state.creator_wallet.keypair,
                ],
                test_state.context.last_blockhash,
            )
        }

        fn get_create_tx(
            test_state: &TestState<S>,
            escrow: &Pubkey,
            escrow_ata: &Pubkey,
        ) -> Transaction {
            let instruction_data =
                InstructionData::data(&cross_chain_escrow_src::instruction::Create {
                    amount: test_state.test_arguments.escrow_amount,
                    order_hash: test_state.order_hash.to_bytes(),
                    hashlock: test_state.hashlock.to_bytes(),
                    recipient: test_state.recipient_wallet.keypair.pubkey(),
                    safety_deposit: test_state.test_arguments.safety_deposit,
                    cancellation_duration: test_state.test_arguments.cancellation_duration,
                    finality_duration: test_state.test_arguments.finality_duration,
                    public_withdrawal_duration: test_state
                        .test_arguments
                        .public_withdrawal_duration,
                    withdrawal_duration: test_state.test_arguments.withdrawal_duration,
                    rescue_start: test_state.test_arguments.rescue_start,
                });

            let instruction: Instruction = Instruction {
                program_id: cross_chain_escrow_src::id(),
                accounts: vec![
                    AccountMeta::new(test_state.payer_kp.pubkey(), true),
                    AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), true),
                    AccountMeta::new_readonly(test_state.token, false),
                    AccountMeta::new(test_state.creator_wallet.token_account, false),
                    AccountMeta::new(*escrow, false),
                    AccountMeta::new(*escrow_ata, false),
                    AccountMeta::new_readonly(spl_associated_token_id, false),
                    AccountMeta::new_readonly(S::get_token_program_id(), false),
                    AccountMeta::new_readonly(rent_id, false),
                    AccountMeta::new_readonly(system_program_id, false),
                ],
                data: instruction_data,
            };
            Transaction::new_signed_with_payer(
                &[instruction],
                Some(&test_state.payer_kp.pubkey()),
                &[
                    &test_state.context.payer,
                    &test_state.creator_wallet.keypair,
                ],
                test_state.context.last_blockhash,
            )
        }

        fn get_withdraw_tx(
            test_state: &TestState<S>,
            escrow: &Pubkey,
            escrow_ata: &Pubkey,
        ) -> Transaction {
            let instruction_data =
                InstructionData::data(&cross_chain_escrow_src::instruction::Withdraw {
                    secret: test_state.secret,
                });

            let instruction: Instruction = Instruction {
                program_id: cross_chain_escrow_src::id(),
                accounts: vec![
                    AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), false),
                    AccountMeta::new_readonly(test_state.recipient_wallet.keypair.pubkey(), true),
                    AccountMeta::new_readonly(test_state.token, false),
                    AccountMeta::new(*escrow, false),
                    AccountMeta::new(*escrow_ata, false),
                    AccountMeta::new(test_state.recipient_wallet.token_account, false),
                    AccountMeta::new_readonly(S::get_token_program_id(), false),
                    AccountMeta::new_readonly(system_program_id, false),
                ],
                data: instruction_data,
            };

            Transaction::new_signed_with_payer(
                &[instruction],
                Some(&test_state.payer_kp.pubkey()),
                &[
                    &test_state.context.payer,
                    &test_state.recipient_wallet.keypair,
                ],
                test_state.context.last_blockhash,
            )
        }

        fn get_rescue_funds_tx(
            test_state: &TestState<S>,
            escrow: &Pubkey,
            token_to_rescue: &Pubkey,
            escrow_ata: &Pubkey,
            recipient_ata: &Pubkey,
        ) -> Transaction {
            let instruction_data =
                InstructionData::data(&cross_chain_escrow_src::instruction::RescueFunds {
                    hashlock: test_state.hashlock.to_bytes(),
                    order_hash: test_state.order_hash.to_bytes(),
                    escrow_creator: test_state.creator_wallet.keypair.pubkey(),
                    escrow_mint: test_state.token,
                    escrow_amount: test_state.test_arguments.escrow_amount,
                    safety_deposit: test_state.test_arguments.safety_deposit,
                    rescue_start: test_state.test_arguments.rescue_start,
                    rescue_amount: test_state.test_arguments.rescue_amount,
                });

            let instruction: Instruction = Instruction {
                program_id: cross_chain_escrow_src::id(),
                accounts: vec![
                    AccountMeta::new(test_state.recipient_wallet.keypair.pubkey(), true),
                    AccountMeta::new_readonly(*token_to_rescue, false),
                    AccountMeta::new(*escrow, false),
                    AccountMeta::new(*escrow_ata, false),
                    AccountMeta::new(*recipient_ata, false),
                    AccountMeta::new_readonly(S::get_token_program_id(), false),
                    AccountMeta::new_readonly(system_program_id, false),
                ],
                data: instruction_data,
            };

            Transaction::new_signed_with_payer(
                &[instruction],
                Some(&test_state.payer_kp.pubkey()),
                &[
                    &test_state.context.payer,
                    &test_state.recipient_wallet.keypair,
                ],
                test_state.context.last_blockhash,
            )
        }

        fn get_escrow_data_len() -> usize {
            cross_chain_escrow_src::constants::DISCRIMINATOR
                + cross_chain_escrow_src::EscrowSrc::INIT_SPACE
        }
    }
}

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
                let (_, _, tx_result) = create_escrow_tx(test_state).await;
                tx_result.expect_error((0, ProgramError::ArithmeticOverflow));
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
            pub async fn test_withdraw(test_state: &mut TestState) {
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
                let transaction = create_public_cancel_tx(test_state, &escrow, &escrow_ata);

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

mod local_helpers {
    use super::*;

    use anchor_lang::InstructionData;
    use solana_program::instruction::{AccountMeta, Instruction};
    use solana_program::pubkey::Pubkey;
    use solana_program::system_program::ID as system_program_id;
    use solana_sdk::signature::Signer;

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
