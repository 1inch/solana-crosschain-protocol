use common::error::EscrowError;
use common_tests::dst_program::DstProgram;
use common_tests::helpers::*;
use common_tests::run_for_tokens;
use common_tests::tests as common_escrow_tests;
use solana_program::program_error::ProgramError;
use solana_program_test::tokio;
use solana_sdk::{signature::Signer, signer::keypair::Keypair, sysvar::clock::Clock};

use test_context::test_context;

run_for_tokens!(
    (TokenSPL, token_spl_tests),
    (Token2022, token_2022_tests) | DstProgram,
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
            async fn test_escrow_creation_fails_with_insufficient_funds(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_escrow_creation_fails_with_insufficient_funds(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_with_zero_safety_deposit(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_escrow_creation_fails_with_zero_safety_deposit(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_with_insufficient_tokens(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_escrow_creation_fails_with_insufficient_tokens(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_with_existing_order_hash(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_escrow_creation_fails_with_existing_order_hash(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_if_finality_duration_overflows(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_escrow_creation_fails_if_finality_duration_overflows(
                    test_state,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_when_cancellation_start_gt_src_cancellation_timestamp(
                test_state: &mut TestState,
            ) {
                let c: Clock = test_state.client.get_sysvar().await.unwrap();
                test_state.test_arguments.src_cancellation_timestamp = c.unix_timestamp as u32 + 1;
                let (_, _, transaction) = create_escrow_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((
                        0,
                        ProgramError::Custom(EscrowError::InvalidCreationTime.into()),
                    ))
            }
        }

        mod test_escrow_withdraw {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw(test_state: &mut TestState) {
                common_escrow_tests::test_withdraw(test_state, test_state.creator_wallet.keypair.pubkey()).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_does_not_work_with_wrong_secret(test_state: &mut TestState) {
                common_escrow_tests::test_withdraw_does_not_work_with_wrong_secret(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_does_not_work_with_non_recipient(test_state: &mut TestState) {
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
                common_escrow_tests::test_withdraw_does_not_work_with_wrong_escrow_ata(test_state, test_state.test_arguments.escrow_amount + 1)
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
            use std::marker::PhantomData;

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

            fn get_token_account_len<S: TokenVariant>(
                _: PhantomData<TestStateBase<DstProgram, S>>,
            ) -> usize {
                S::get_token_account_size()
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_tokens_by_creator(test_state: &mut TestState) {
                let (escrow, escrow_ata) = create_escrow(test_state).await;

                let transaction = DstProgram::get_public_withdraw_tx(
                    test_state,
                    &escrow,
                    &escrow_ata,
                    &test_state.creator_wallet.keypair,
                );

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
                );

                // Check that the escrow balance is correct
                assert_eq!(
                    get_token_balance(&mut test_state.context, &escrow_ata).await,
                    test_state.test_arguments.escrow_amount
                );
                let escrow_data_len =
                    <DstProgram as EscrowVariant<Token2022>>::get_escrow_data_len();
                let rent_lamports =
                    get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;
                let token_account_rent = get_min_rent_for_size(
                    &mut test_state.client,
                    get_token_account_len(PhantomData::<TestState>),
                )
                .await;
                assert_eq!(
                    rent_lamports,
                    test_state.client.get_balance(escrow).await.unwrap()
                );

                test_state
                    .expect_balance_change(
                        transaction,
                        &[
                            native_change(
                                test_state.creator_wallet.keypair.pubkey(),
                                rent_lamports + token_account_rent,
                            ),
                            token_change(
                                test_state.recipient_wallet.token_account,
                                test_state.test_arguments.escrow_amount,
                            ),
                        ],
                    )
                    .await;

                // Assert accounts were closed
                assert!(test_state
                    .client
                    .get_account(escrow)
                    .await
                    .unwrap()
                    .is_none());

                // Assert escrow_ata was closed
                assert!(test_state
                    .client
                    .get_account(escrow_ata)
                    .await
                    .unwrap()
                    .is_none());
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
                let rent_recipient = test_state.creator_wallet.keypair.pubkey();
                common_escrow_tests::test_public_withdraw_tokens(test_state, withdrawer, rent_recipient).await
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
                let new_escrow_amount = test_state.test_arguments.escrow_amount + 1;
                common_escrow_tests::test_public_withdraw_fails_with_wrong_escrow_ata(test_state, new_escrow_amount)
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

// Native Mint (wrapped SOL) is always owned by the SPL Token program
type TestState = TestStateBase<DstProgram, TokenSPL>;
// Tests for native token (SOL)
mod test_escrow_native {
    use anchor_spl::token::spl_token::native_mint::ID as NATIVE_MINT;

    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        common_escrow_tests::test_escrow_creation_native(
            test_state,
            test_state.creator_wallet.keypair.pubkey(),
        )
        .await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fails_if_token_is_not_native(test_state: &mut TestState) {
        test_state.test_arguments.asset_is_native = true;
        common_escrow_tests::test_escrow_creation_fails_if_token_is_not_native(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        common_escrow_tests::test_withdraw(test_state, test_state.creator_wallet.keypair.pubkey()).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_by_resolver(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        let withdrawer = test_state.recipient_wallet.keypair.insecure_clone();
        let rent_recipient = test_state.creator_wallet.keypair.pubkey();
        common_escrow_tests::test_public_withdraw_tokens(test_state, withdrawer, rent_recipient).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_by_any_account(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        let withdrawer = Keypair::new();
        let payer_kp = &test_state.payer_kp;
        let context = &mut test_state.context;

        transfer_lamports(
            context,
            WALLET_DEFAULT_LAMPORTS,
            payer_kp,
            &withdrawer.pubkey(),
        )
        .await;
        let rent_recipient = test_state.creator_wallet.keypair.pubkey();
        common_escrow_tests::test_public_withdraw_tokens(test_state, withdrawer, rent_recipient).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        common_escrow_tests::test_cancel_native(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_all_tokens_and_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        common_escrow_tests::test_rescue_all_tokens_and_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_part_of_tokens_and_not_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        common_escrow_tests::test_rescue_part_of_tokens_and_not_close_ata(test_state).await
    }
}

// Tests for wrapped native mint (WSOL)
mod test_escrow_wrapped_native {
    use anchor_spl::token::spl_token::native_mint::ID as NATIVE_MINT;

    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        common_escrow_tests::test_escrow_creation(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        common_escrow_tests::test_withdraw(test_state, test_state.creator_wallet.keypair.pubkey()).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_by_resolver(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        let withdrawer = test_state.recipient_wallet.keypair.insecure_clone();
        let rent_recipient = test_state.creator_wallet.keypair.pubkey();
        common_escrow_tests::test_public_withdraw_tokens(test_state, withdrawer, rent_recipient).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_by_any_account(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        let withdrawer = Keypair::new();
        let payer_kp = &test_state.payer_kp;
        let context = &mut test_state.context;

        transfer_lamports(
            context,
            WALLET_DEFAULT_LAMPORTS,
            payer_kp,
            &withdrawer.pubkey(),
        )
        .await;
        let rent_recipient = test_state.creator_wallet.keypair.pubkey();
        common_escrow_tests::test_public_withdraw_tokens(test_state, withdrawer, rent_recipient).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        common_escrow_tests::test_cancel(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_all_tokens_and_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        common_escrow_tests::test_rescue_all_tokens_and_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_part_of_tokens_and_not_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        common_escrow_tests::test_rescue_part_of_tokens_and_not_close_ata(test_state).await
    }
}

mod test_escrow_creation_cost {
    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_tx_cost(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation_tx_cost(test_state).await
    }
}
