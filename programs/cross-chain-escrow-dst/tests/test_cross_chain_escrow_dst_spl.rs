use anchor_lang::error::ErrorCode;
use common::{error::EscrowError, timelocks::Stage};
use common_tests::dst_program::DstProgram;
use common_tests::helpers::*;
use common_tests::run_for_tokens;
use common_tests::tests as common_escrow_tests;
use common_tests::whitelist::prepare_resolvers;
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
            async fn test_escrow_creation_with_pre_existing_escrow_ata(test_state: &mut TestState) {
                let (escrow_pda, _) =
                    get_escrow_addresses(test_state, test_state.taker_wallet.keypair.pubkey());

                let _escrow_ata =
                    <TestState as HasTokenVariant>::Token::initialize_spl_associated_account(
                        &mut test_state.context,
                        &test_state.token,
                        &escrow_pda,
                    )
                    .await;
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
                    .expect_error(ProgramError::Custom(
                        EscrowError::InvalidCreationTime.into(),
                    ))
            }
        }
        mod test_escrow_withdraw {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw(test_state: &mut TestState) {
                let rent_recipient = test_state.maker_wallet.keypair.pubkey();
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                let transaction = DstProgram::get_withdraw_tx(test_state, &escrow, &escrow_ata);

                let token_account_rent = get_min_rent_for_size(
                    &mut test_state.client,
                    <TestState as HasTokenVariant>::Token::get_token_account_size(),
                )
                .await;

                let escrow_rent =
                    get_min_rent_for_size(&mut test_state.client, DEFAULT_DST_ESCROW_SIZE).await;

                set_time(
                    &mut test_state.context,
                    test_state
                        .test_arguments
                        .dst_timelocks
                        .get(Stage::DstWithdrawal)
                        .unwrap(),
                );

                let (_, taker_ata) = find_user_ata(test_state);

                test_state
                    .expect_state_change(
                        transaction,
                        &[
                            native_change(rent_recipient, token_account_rent + escrow_rent),
                            token_change(taker_ata, test_state.test_arguments.escrow_amount),
                        ],
                    )
                    .await;

                // Assert escrow was closed
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
            async fn test_withdraw_with_excess_tokens(test_state: &mut TestState) {
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                let transaction = DstProgram::get_withdraw_tx(test_state, &escrow, &escrow_ata);

                set_time(
                    &mut test_state.context,
                    test_state
                        .test_arguments
                        .dst_timelocks
                        .get(Stage::DstWithdrawal)
                        .unwrap(),
                );

                let (_, taker_ata) = find_user_ata(test_state);

                let excess_amount = 1000;
                // Send excess tokens to the escrow account
                mint_excess_tokens(test_state, &escrow_ata, excess_amount).await;
                test_state
                    .expect_state_change(
                        transaction,
                        &[token_change(
                            taker_ata,
                            test_state.test_arguments.escrow_amount + excess_amount,
                        )],
                    )
                    .await;

                // Assert escrow was closed
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
            async fn test_withdraw_does_not_work_with_wrong_taker_ata(test_state: &mut TestState) {
                common_escrow_tests::test_withdraw_does_not_work_with_wrong_taker_ata(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_does_not_work_with_wrong_escrow_ata(test_state: &mut TestState) {
                let new_escrow_amount = test_state.test_arguments.escrow_amount + 1;
                common_escrow_tests::test_withdraw_does_not_work_with_wrong_escrow_ata(
                    test_state,
                    new_escrow_amount,
                )
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

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_fails_with_incorrect_token(test_state: &mut TestState) {
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                test_state.token = <TestState as HasTokenVariant>::Token::deploy_spl_token(
                    &mut test_state.context,
                )
                .await
                .pubkey();

                let transaction = DstProgram::get_withdraw_tx(test_state, &escrow, &escrow_ata);
                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(ErrorCode::ConstraintSeeds.into()));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_fails_if_withdrawal_duration_overflows(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.dst_timelocks =
                    init_timelocks(0, 0, 0, 0, u32::MAX, 0, 0, 0);
                let (escrow, escrow_ata) = create_escrow(test_state).await;

                let transaction = DstProgram::get_withdraw_tx(test_state, &escrow, &escrow_ata);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::ArithmeticOverflow);
            }
        }

        mod test_escrow_public_withdraw {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_before_start_of_public_withdraw(
                test_state: &mut TestState,
            ) {
                prepare_resolvers(test_state, &[test_state.context.payer.pubkey()]).await;
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
                prepare_resolvers(test_state, &[test_state.context.payer.pubkey()]).await;
                common_escrow_tests::test_public_withdraw_fails_after_cancellation_start(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_tokens_by_maker(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.maker_wallet.keypair.pubkey()]).await;
                let (escrow, escrow_ata) = create_escrow(test_state).await;

                let transaction = DstProgram::get_public_withdraw_tx(
                    test_state,
                    &escrow,
                    &escrow_ata,
                    &test_state.maker_wallet.keypair,
                );

                set_time(
                    &mut test_state.context,
                    test_state
                        .test_arguments
                        .dst_timelocks
                        .get(Stage::DstPublicWithdrawal)
                        .unwrap(),
                );

                let escrow_data_len = DEFAULT_DST_ESCROW_SIZE;

                let rent_lamports =
                    get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

                let token_account_rent = get_min_rent_for_size(
                    &mut test_state.client,
                    <TestState as HasTokenVariant>::Token::get_token_account_size(),
                )
                .await;

                assert_eq!(
                    rent_lamports,
                    test_state.client.get_balance(escrow).await.unwrap()
                );

                test_state
                    .expect_state_change(
                        transaction,
                        &[
                            native_change(
                                test_state.maker_wallet.keypair.pubkey(),
                                rent_lamports + token_account_rent,
                            ),
                            token_change(
                                test_state.taker_wallet.token_account,
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
            async fn test_public_withdraw_tokens_by_any_resolver(test_state: &mut TestState) {
                let withdrawer = Keypair::new();
                prepare_resolvers(test_state, &[withdrawer.pubkey()]).await;
                transfer_lamports(
                    &mut test_state.context,
                    WALLET_DEFAULT_LAMPORTS,
                    &test_state.payer_kp,
                    &withdrawer.pubkey(),
                )
                .await;

                let (escrow, escrow_ata) = create_escrow(test_state).await;

                let transaction = DstProgram::get_public_withdraw_tx(
                    test_state,
                    &escrow,
                    &escrow_ata,
                    &withdrawer,
                );

                set_time(
                    &mut test_state.context,
                    test_state
                        .test_arguments
                        .dst_timelocks
                        .get(Stage::DstPublicWithdrawal)
                        .unwrap(),
                );

                // Check that the escrow balance is correct
                assert_eq!(
                    get_token_balance(&mut test_state.context, &escrow_ata).await,
                    test_state.test_arguments.escrow_amount
                );

                let escrow_data_len = DEFAULT_DST_ESCROW_SIZE;

                let rent_lamports =
                    get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

                let token_account_rent = get_min_rent_for_size(
                    &mut test_state.client,
                    <TestState as HasTokenVariant>::Token::get_token_account_size(),
                )
                .await;

                assert_eq!(
                    rent_lamports,
                    test_state.client.get_balance(escrow).await.unwrap()
                );

                let (_, taker_ata) = find_user_ata(test_state);

                test_state
                    .expect_state_change(
                        transaction,
                        &[
                            native_change(
                                test_state.maker_wallet.keypair.pubkey(),
                                token_account_rent + rent_lamports
                                    - test_state.test_arguments.safety_deposit,
                            ),
                            native_change(
                                withdrawer.pubkey(),
                                test_state.test_arguments.safety_deposit,
                            ),
                            token_change(taker_ata, test_state.test_arguments.escrow_amount),
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
            async fn test_public_withdraw_fails_if_public_withdrawal_duration_overflows(
                test_state: &mut TestState,
            ) {
                prepare_resolvers(test_state, &[test_state.maker_wallet.keypair.pubkey()]).await;
                test_state.test_arguments.dst_timelocks =
                    init_timelocks(0, 0, 0, 0, 0, u32::MAX, 0, 0);
                let (escrow, escrow_ata) = create_escrow(test_state).await;

                let transaction = DstProgram::get_public_withdraw_tx(
                    test_state,
                    &escrow,
                    &escrow_ata,
                    &test_state.maker_wallet.keypair,
                );

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::ArithmeticOverflow);
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_with_wrong_secret(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.context.payer.pubkey()]).await;
                common_escrow_tests::test_public_withdraw_fails_with_wrong_secret(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_with_wrong_taker_ata(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_public_withdraw_fails_with_wrong_taker_ata(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_with_wrong_escrow_ata(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let new_escrow_amount = test_state.test_arguments.escrow_amount + 1;
                common_escrow_tests::test_public_withdraw_fails_with_wrong_escrow_ata(
                    test_state,
                    new_escrow_amount,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_without_resolver_access(
                test_state: &mut TestState,
            ) {
                let (escrow, escrow_ata) = create_escrow(test_state).await;

                let withdrawer = Keypair::new();
                transfer_lamports(
                    &mut test_state.context,
                    WALLET_DEFAULT_LAMPORTS,
                    &test_state.payer_kp,
                    &withdrawer.pubkey(),
                )
                .await;

                // withdrawer does not have resolver access
                let transaction = DstProgram::get_public_withdraw_tx(
                    test_state,
                    &escrow,
                    &escrow_ata,
                    &withdrawer,
                );

                set_time(
                    &mut test_state.context,
                    test_state
                        .test_arguments
                        .dst_timelocks
                        .get(Stage::DstPublicWithdrawal)
                        .unwrap(),
                );

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(
                        ErrorCode::AccountNotInitialized.into(),
                    ));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_with_incorrect_token(test_state: &mut TestState) {
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_state.token = <TestState as HasTokenVariant>::Token::deploy_spl_token(
                    &mut test_state.context,
                )
                .await
                .pubkey();
                let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

                let transaction =
                    DstProgram::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &taker_kp);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(ErrorCode::ConstraintSeeds.into()));
            }
        }

        mod test_escrow_cancel {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel(test_state: &mut TestState) {
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                common_escrow_tests::test_cancel(test_state, &escrow, &escrow_ata).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_with_excess_tokens(test_state: &mut TestState) {
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                let transaction = DstProgram::get_cancel_tx(test_state, &escrow, &escrow_ata);

                set_time(
                    &mut test_state.context,
                    test_state
                        .test_arguments
                        .dst_timelocks
                        .get(Stage::DstCancellation)
                        .unwrap(),
                );

                let (maker_ata, _) = find_user_ata(test_state);

                let excess_amount = 1000;
                // Send excess tokens to the escrow account
                mint_excess_tokens(test_state, &escrow_ata, excess_amount).await;

                test_state
                    .expect_state_change(
                        transaction,
                        &[
                            token_change(
                                maker_ata,
                                test_state.test_arguments.escrow_amount + excess_amount,
                            ),
                            account_closure(escrow_ata, true),
                            account_closure(escrow, true),
                        ],
                    )
                    .await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_cancel_by_non_maker(test_state: &mut TestState) {
                common_escrow_tests::test_cannot_cancel_by_non_maker(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_cancel_with_wrong_maker_ata(test_state: &mut TestState) {
                common_escrow_tests::test_cannot_cancel_with_wrong_maker_ata(test_state).await
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

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_fails_with_incorrect_token(test_state: &mut TestState) {
                let (escrow, escrow_ata) = create_escrow(test_state).await;

                test_state.token = <TestState as HasTokenVariant>::Token::deploy_spl_token(
                    &mut test_state.context,
                )
                .await
                .pubkey();

                let transaction = DstProgram::get_cancel_tx(test_state, &escrow, &escrow_ata);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(ErrorCode::ConstraintSeeds.into()));
            }
        }
        mod test_escrow_rescue_funds {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_rescue_all_tokens_and_close_ata(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_rescue_all_tokens_and_close_ata(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_rescue_part_of_tokens_and_not_close_ata(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_rescue_part_of_tokens_and_not_close_ata(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_rescue_tokens_when_escrow_is_deleted(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_rescue_tokens_when_escrow_is_deleted(test_state).await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_before_rescue_delay_pass(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_cannot_rescue_funds_before_rescue_delay_pass(test_state)
                    .await
            }

            // TODO: Replace with a test that non-creator cannot rescue funds
            // #[test_context(TestState)]
            // #[tokio::test]
            // async fn test_cannot_rescue_funds_by_non_recipient(test_state: &mut TestState) {
            //     prepare_resolvers(test_state, &[test_state.maker_wallet.keypair.pubkey()]).await;
            //     common_escrow_tests::test_cannot_rescue_funds_by_non_recipient(test_state).await
            // }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_with_wrong_taker_ata(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_cannot_rescue_funds_with_wrong_taker_ata(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_with_wrong_escrow_ata(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_cannot_rescue_funds_with_wrong_escrow_ata(test_state)
                    .await
            }
        }
    }
);

// pub async fn test_cannot_rescue_funds_by_non_whitelisted_resolver<S: TokenVariant>(
//     test_state: &mut TestStateBase<DstProgram, S>,
// ) {
//     let (escrow, _) = create_escrow(test_state).await;

//     let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
//     let escrow_ata = S::initialize_spl_associated_account(
//         &mut test_state.context,
//         &token_to_rescue,
//         &escrow,
//     )
//     .await;
//     let maker_ata = S::initialize_spl_associated_account(
//         &mut test_state.context,
//         &token_to_rescue,
//         &test_state.maker_wallet.keypair.pubkey(),
//     )
//     .await;

//     S::mint_spl_tokens(
//         &mut test_state.context,
//         &token_to_rescue,
//         &escrow_ata,
//         &test_state.payer_kp.pubkey(),
//         &test_state.payer_kp,
//         test_state.test_arguments.rescue_amount,
//     )
//     .await;

//     let transaction = DstProgram::get_rescue_funds_tx(
//         test_state,
//         &escrow,
//         &token_to_rescue,
//         &escrow_ata,
//         &maker_ata,
//     );

//     set_time(
//         &mut test_state.context,
//         test_state.init_timestamp + RESCUE_DELAY + 100,
//     );
//     test_state
//         .client
//         .process_transaction(transaction)
//         .await
//         .expect_error((
//             0,
//             ProgramError::Custom(ErrorCode::AccountNotInitialized.into()),
//         ));
// }
