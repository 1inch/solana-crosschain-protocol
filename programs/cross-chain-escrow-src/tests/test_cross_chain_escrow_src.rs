use anchor_lang::{error::ErrorCode, prelude::ProgramError};
use anchor_spl::token::spl_token::native_mint::ID as NATIVE_MINT;
use anchor_spl::token::spl_token::state::Account as SplTokenAccount;
use common::error::EscrowError;
use common_tests::helpers::*;
use common_tests::run_for_tokens;
use common_tests::src_program::{
    create_order, create_order_data, get_cancel_order_by_resolver_tx, get_cancel_order_tx,
    get_create_order_tx, get_order_addresses, get_order_data_len, SrcProgram,
};
use common_tests::tests as common_escrow_tests;
use cross_chain_escrow_src::calculate_premium;
use solana_program_test::tokio;
use solana_sdk::clock::Clock;
use solana_sdk::signature::Signer;
use solana_sdk::signer::keypair::Keypair;
use test_context::test_context;

mod merkle_tree_test_helpers;
use merkle_tree_test_helpers::{get_proof, get_root};

run_for_tokens!(
    (TokenSPL, token_spl_tests),
    (Token2022, token_2022_tests) | SrcProgram,
    mod token_module {

        use super::*;

        mod test_order_creation {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation(test_state: &mut TestState) {
                local_helpers::test_order_creation(test_state).await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fails_with_zero_amount(test_state: &mut TestState) {
                test_state.test_arguments.order_amount = 0;
                let (_, _, transaction) = create_order_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((
                        0,
                        ProgramError::Custom(EscrowError::ZeroAmountOrDeposit.into()),
                    ));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fails_with_zero_safety_deposit(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.safety_deposit = 0;
                let (_, _, transaction) = create_order_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((
                        0,
                        ProgramError::Custom(EscrowError::ZeroAmountOrDeposit.into()),
                    ));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fails_with_insufficient_funds(test_state: &mut TestState) {
                test_state.test_arguments.safety_deposit = WALLET_DEFAULT_LAMPORTS + 1;

                let (_, _, transaction) = create_order_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((
                        0,
                        ProgramError::Custom(EscrowError::SafetyDepositTooLarge.into()),
                    ));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fails_with_existing_order_hash(
                test_state: &mut TestState,
            ) {
                let (_, _, mut transaction) = create_order_data(test_state);

                // Send the transaction.
                test_state
                    .client
                    .process_transaction(transaction.clone())
                    .await
                    .expect_success();
                let new_hash = test_state.context.get_new_latest_blockhash().await.unwrap();

                if transaction.signatures.len() == 1 {
                    transaction.sign(&[&test_state.creator_wallet.keypair], new_hash);
                }
                if transaction.signatures.len() == 2 {
                    transaction.sign(
                        &[
                            &test_state.creator_wallet.keypair,
                            &test_state.context.payer,
                        ],
                        new_hash,
                    );
                }
                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((
                        0,
                        ProgramError::Custom(
                            solana_sdk::system_instruction::SystemError::AccountAlreadyInUse as u32,
                        ),
                    ));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fails_with_zero_expiration_duration(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.expiration_duration = 0;
                let (order, order_ata, tx) = create_order_data(test_state);

                test_state
                    .client
                    .process_transaction(tx)
                    .await
                    .expect_error((0, ProgramError::Custom(EscrowError::InvalidTime.into())));

                // Check that the order accounts have not been created.
                let acc_lookup_result = test_state.client.get_account(order).await.unwrap();
                assert!(acc_lookup_result.is_none());

                let acc_lookup_result = test_state.client.get_account(order_ata).await.unwrap();
                assert!(acc_lookup_result.is_none());
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fails_if_fee_is_greater_than_lamport_balance(
                test_state: &mut TestState,
            ) {
                let (order, order_ata) = get_order_addresses(test_state);

                let token_account_rent = get_min_rent_for_size(
                    &mut test_state.client,
                    local_helpers::get_token_account_len(std::marker::PhantomData::<TestState>),
                )
                .await;

                test_state.test_arguments.max_cancellation_premium = token_account_rent + 1;

                let transaction = get_create_order_tx(test_state, &order, &order_ata);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((
                        0,
                        ProgramError::Custom(EscrowError::InvalidCancellationFee.into()),
                    ));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fails_with_incorrect_parts_without_allow_multiples_fills(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.order_parts_amount = 2;
                let (_, _, transaction) = create_order_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((
                        0,
                        ProgramError::Custom(EscrowError::InvalidPartsAmount.into()),
                    ));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fails_with_zero_parts_without_allow_multiples_fills(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.order_parts_amount = 0;
                let (_, _, transaction) = create_order_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((
                        0,
                        ProgramError::Custom(EscrowError::InvalidPartsAmount.into()),
                    ));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fails_with_zero_parts_for_allow_multiples_fills(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.order_parts_amount = 0;
                test_state.test_arguments.allow_multiple_fills = true;
                let (_, _, transaction) = create_order_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((
                        0,
                        ProgramError::Custom(EscrowError::InvalidPartsAmount.into()),
                    ));
            }
        }

        mod test_escrow_creation {
            use super::*;

            const AUCTION_START_OFFSET: u32 = 250;
            const AUCTION_DURATION: u32 = 1000;
            const INITIAL_RATE_BUMP: u16 = 10_000; // 10%
            const INTERMEDIATE_RATE_BUMP: u16 = 9_000; // 9%
            const INTERMEDIATE_TIME_DELTA: u16 = 500;
            const EXPECTED_MULTIPLIER_NUMERATOR: u64 = 1095;
            const EXPECTED_MULTIPLIER_DENOMINATOR: u64 = 1000;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation(test_state: &mut TestState) {
                create_order(test_state).await;
                common_escrow_tests::test_escrow_creation(test_state).await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_with_dutch_auction_params(test_state: &mut TestState) {
                test_state.test_arguments.dutch_auction_data =
                    cross_chain_escrow_src::AuctionData {
                        start_time: test_state.init_timestamp - AUCTION_START_OFFSET,
                        duration: AUCTION_DURATION,
                        initial_rate_bump: INITIAL_RATE_BUMP,
                        points_and_time_deltas: vec![
                            cross_chain_escrow_src::auction::PointAndTimeDelta {
                                rate_bump: INTERMEDIATE_RATE_BUMP,
                                time_delta: INTERMEDIATE_TIME_DELTA,
                            },
                        ],
                    };

                create_order(test_state).await;
                common_escrow_tests::test_escrow_creation(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_with_wrong_dutch_auction_hash(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.dutch_auction_data =
                    cross_chain_escrow_src::AuctionData {
                        start_time: test_state.init_timestamp - AUCTION_START_OFFSET,
                        duration: AUCTION_DURATION,
                        initial_rate_bump: INITIAL_RATE_BUMP,
                        points_and_time_deltas: vec![
                            cross_chain_escrow_src::auction::PointAndTimeDelta {
                                rate_bump: INTERMEDIATE_RATE_BUMP,
                                time_delta: INTERMEDIATE_TIME_DELTA,
                            },
                        ],
                    };

                create_order(test_state).await;
                test_state.test_arguments.dutch_auction_data =
                    cross_chain_escrow_src::AuctionData {
                        start_time: test_state.init_timestamp - AUCTION_START_OFFSET,
                        duration: AUCTION_DURATION,
                        initial_rate_bump: INITIAL_RATE_BUMP,
                        points_and_time_deltas: vec![
                            cross_chain_escrow_src::auction::PointAndTimeDelta {
                                rate_bump: INTERMEDIATE_RATE_BUMP * 2, // Incorrect rate bump
                                time_delta: INTERMEDIATE_TIME_DELTA,
                            },
                        ],
                    };
                let (_, _, tx) = create_escrow_data(test_state);
                test_state
                    .client
                    .process_transaction(tx)
                    .await
                    .expect_error((
                        0,
                        ProgramError::Custom(EscrowError::DutchAuctionDataHashMismatch.into()),
                    ));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_calculation_of_dutch_auction_params(test_state: &mut TestState) {
                test_state.test_arguments.dutch_auction_data =
                    cross_chain_escrow_src::AuctionData {
                        start_time: test_state.init_timestamp - AUCTION_START_OFFSET,
                        duration: AUCTION_DURATION,
                        initial_rate_bump: INITIAL_RATE_BUMP,
                        points_and_time_deltas: vec![
                            cross_chain_escrow_src::auction::PointAndTimeDelta {
                                rate_bump: INTERMEDIATE_RATE_BUMP, // 9%
                                time_delta: INTERMEDIATE_TIME_DELTA,
                            },
                        ],
                    };

                create_order(test_state).await;

                let (escrow, _) = create_escrow(test_state).await;
                let escrow_account_data = test_state
                    .client
                    .get_account(escrow)
                    .await
                    .unwrap()
                    .unwrap()
                    .data;
                let dst_amount = local_helpers::get_dst_amount(&escrow_account_data)
                    .expect("Failed to read dst_amount from escrow account data");

                assert_eq!(
                    dst_amount,
                    test_state.test_arguments.dst_amount * EXPECTED_MULTIPLIER_NUMERATOR
                        / EXPECTED_MULTIPLIER_DENOMINATOR,
                );
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_with_empty_order_account(
                test_state: &mut TestState,
            ) {
                // Create an escrow account without existing order account.
                let (escrow, escrow_ata, tx) = create_escrow_data(test_state);

                test_state
                    .client
                    .process_transaction(tx)
                    .await
                    .expect_error((
                        0,
                        ProgramError::Custom(ErrorCode::AccountNotInitialized.into()),
                    ));

                // Check that the order accounts have not been created.
                let acc_lookup_result = test_state.client.get_account(escrow).await.unwrap();
                assert!(acc_lookup_result.is_none());

                let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
                assert!(acc_lookup_result.is_none());
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_if_finality_duration_overflows(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.finality_duration = u32::MAX;
                create_order(test_state).await;
                common_escrow_tests::test_escrow_creation_fails_if_finality_duration_overflows(
                    test_state,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_if_withdrawal_duration_overflows(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.withdrawal_duration = u32::MAX;
                create_order(test_state).await;
                common_escrow_tests::test_escrow_creation_fails_if_withdrawal_duration_overflows(
                    test_state,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_if_public_withdrawal_duration_overflows(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.public_withdrawal_duration = u32::MAX;
                create_order(test_state).await;
                common_escrow_tests::test_escrow_creation_fails_if_public_withdrawal_duration_overflows(
                    test_state,
                )
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_if_cancellation_duration_overflows(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.cancellation_duration = u32::MAX;
                create_order(test_state).await;
                let (_, _, transaction) = create_escrow_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((0, ProgramError::ArithmeticOverflow));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_with_expired_order(test_state: &mut TestState) {
                create_order(test_state).await;

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp + test_state.test_arguments.expiration_duration + 1,
                );

                let (_, _, transaction) = create_escrow_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((0, ProgramError::Custom(EscrowError::OrderHasExpired.into())));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_when_escrow_amount_is_too_large(
                test_state: &mut TestState,
            ) {
                create_order(test_state).await;
                test_state.test_arguments.escrow_amount =
                    test_state.test_arguments.order_amount + 1;
                let (_, _, transaction) = create_escrow_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((0, ProgramError::Custom(EscrowError::InvalidAmount.into())));
            }
        }

        mod test_escrow_withdraw {
            use super::*;
            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_only(test_state: &mut TestState) {
                create_order(test_state).await;
                let rent_recipient = test_state.recipient_wallet.keypair.pubkey();
                common_escrow_tests::test_withdraw(test_state, rent_recipient).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_does_not_work_with_wrong_secret(test_state: &mut TestState) {
                create_order(test_state).await;
                common_escrow_tests::test_withdraw_does_not_work_with_wrong_secret(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_does_not_work_with_non_recipient(test_state: &mut TestState) {
                create_order(test_state).await;
                common_escrow_tests::test_withdraw_does_not_work_with_non_recipient(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_does_not_work_with_wrong_recipient_ata(
                test_state: &mut TestState,
            ) {
                create_order(test_state).await;
                common_escrow_tests::test_withdraw_does_not_work_with_wrong_recipient_ata(
                    test_state,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_does_not_work_with_wrong_escrow_ata(test_state: &mut TestState) {
                let diff_amount = 1;
                let new_amount = test_state.test_arguments.order_amount + diff_amount;
                test_state.test_arguments.order_amount = new_amount;
                create_order(test_state).await;
                test_state.test_arguments.order_amount -= diff_amount;
                create_order(test_state).await;
                common_escrow_tests::test_withdraw_does_not_work_with_wrong_escrow_ata(
                    test_state, new_amount,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_does_not_work_before_withdrawal_start(
                test_state: &mut TestState,
            ) {
                create_order(test_state).await;
                common_escrow_tests::test_withdraw_does_not_work_before_withdrawal_start(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_does_not_work_after_cancellation_start(
                test_state: &mut TestState,
            ) {
                create_order(test_state).await;
                common_escrow_tests::test_withdraw_does_not_work_after_cancellation_start(
                    test_state,
                )
                .await
            }
        }

        mod test_order_public_withdraw {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_tokens_by_recipient(test_state: &mut TestState) {
                create_order(test_state).await;
                let rent_recipient = test_state.recipient_wallet.keypair.pubkey();
                common_escrow_tests::test_public_withdraw_tokens(
                    test_state,
                    test_state.recipient_wallet.keypair.insecure_clone(),
                    rent_recipient,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_tokens_by_any_account(test_state: &mut TestState) {
                create_order(test_state).await;
                let withdrawer = Keypair::new();
                transfer_lamports(
                    &mut test_state.context,
                    WALLET_DEFAULT_LAMPORTS,
                    &test_state.payer_kp,
                    &withdrawer.pubkey(),
                )
                .await;
                let rent_recipient = test_state.recipient_wallet.keypair.pubkey();
                common_escrow_tests::test_public_withdraw_tokens(
                    test_state,
                    withdrawer,
                    rent_recipient,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_with_wrong_secret(test_state: &mut TestState) {
                create_order(test_state).await;
                common_escrow_tests::test_public_withdraw_fails_with_wrong_secret(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_with_wrong_recipient_ata(
                test_state: &mut TestState,
            ) {
                create_order(test_state).await;
                common_escrow_tests::test_public_withdraw_fails_with_wrong_recipient_ata(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_with_wrong_escrow_ata(test_state: &mut TestState) {
                let diff = 1;
                let new_amount = test_state.test_arguments.order_amount + diff;
                test_state.test_arguments.order_amount = new_amount;
                create_order(test_state).await;
                test_state.test_arguments.order_amount -= diff;
                create_order(test_state).await;
                common_escrow_tests::test_public_withdraw_fails_with_wrong_escrow_ata(
                    test_state, new_amount,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_before_start_of_public_withdraw(
                test_state: &mut TestState,
            ) {
                create_order(test_state).await;
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
                create_order(test_state).await;
                common_escrow_tests::test_public_withdraw_fails_after_cancellation_start(test_state)
                    .await
            }
        }

        mod test_escrow_cancel {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel(test_state: &mut TestState) {
                create_order(test_state).await;
                common_escrow_tests::test_cancel(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_cancel_by_non_creator(test_state: &mut TestState) {
                create_order(test_state).await;
                common_escrow_tests::test_cannot_cancel_by_non_creator(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_cancel_with_wrong_creator_ata(test_state: &mut TestState) {
                create_order(test_state).await;
                common_escrow_tests::test_cannot_cancel_with_wrong_creator_ata(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_cancel_with_wrong_escrow_ata(test_state: &mut TestState) {
                create_order(test_state).await;
                let (escrow, _) = create_escrow(test_state).await;

                test_state.test_arguments.order_amount += 1;
                test_state.test_arguments.escrow_amount += 1;
                create_order(test_state).await;

                let (_, escrow_ata_2) = create_escrow(test_state).await;

                let transaction = SrcProgram::get_cancel_tx(test_state, &escrow, &escrow_ata_2);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((
                        0,
                        ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()),
                    ))
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_cancel_before_cancellation_start(test_state: &mut TestState) {
                create_order(test_state).await;
                common_escrow_tests::test_cannot_cancel_before_cancellation_start(test_state).await
            }
        }

        mod test_order_cancel {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_cancel(test_state: &mut TestState) {
                local_helpers::test_order_cancel(test_state).await;
            }
        }

        mod test_order_cancel_by_resolver {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_by_resolver_for_free_at_the_auction_start(
                test_state: &mut TestState,
            ) {
                local_helpers::test_cancel_by_resolver_for_free_at_the_auction_start(test_state)
                    .await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_by_resolver_at_different_points(test_state: &mut TestState) {
                local_helpers::test_cancel_by_resolver_at_different_points(test_state, false, None)
                    .await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_by_resolver_after_auction(test_state: &mut TestState) {
                local_helpers::test_cancel_by_resolver_after_auction(test_state).await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_by_resolver_reward_less_then_auction_calculated(
                test_state: &mut TestState,
            ) {
                local_helpers::test_cancel_by_resolver_reward_less_then_auction_calculated(
                    test_state,
                )
                .await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_by_resolver_fails_if_order_is_not_expired(
                test_state: &mut TestState,
            ) {
                let (order, order_ata) = create_order(test_state).await;

                let transaction =
                    get_cancel_order_by_resolver_tx(test_state, &order, &order_ata, None);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((0, ProgramError::Custom(EscrowError::OrderNotExpired.into())));
            }
        }

        mod test_order_public_cancel {
            use super::local_helpers::*;
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_cancel_by_taker(test_state: &mut TestState) {
                test_public_cancel_escrow(
                    test_state,
                    &test_state.recipient_wallet.keypair.insecure_clone(),
                )
                .await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_cancel_by_any_account(test_state: &mut TestState) {
                let canceller = Keypair::new();
                transfer_lamports(
                    &mut test_state.context,
                    WALLET_DEFAULT_LAMPORTS,
                    &test_state.payer_kp,
                    &canceller.pubkey(),
                )
                .await;

                test_public_cancel_escrow(test_state, &canceller).await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_public_cancel_before_public_cancellation_start(
                test_state: &mut TestState,
            ) {
                create_order(test_state).await;
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                let transaction = create_public_escrow_cancel_tx(
                    test_state,
                    &escrow,
                    &escrow_ata,
                    &test_state.payer_kp,
                );

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

        mod test_order_rescue_funds_for_order {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_rescue_all_tokens_from_order_and_close_ata(
                test_state: &mut TestState,
            ) {
                local_helpers::test_rescue_all_tokens_from_order_and_close_ata(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_rescue_part_of_tokens_from_order_and_not_close_ata(
                test_state: &mut TestState,
            ) {
                local_helpers::test_rescue_part_of_tokens_from_order_and_not_close_ata(
                    test_state,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_from_order_before_rescue_delay_pass(
                test_state: &mut TestState,
            ) {
                local_helpers::test_cannot_rescue_funds_from_order_before_rescue_delay_pass(
                    test_state,
                )
                .await
            }

            // #[test_context(TestState)]
            // #[tokio::test]
            // async fn test_cannot_rescue_funds_from_order_by_non_recipient(test_state: &mut TestState) { // TODO: return after implement whitelist
            //     local_helpers::test_cannot_rescue_funds_from_order_by_non_recipient(test_state).await
            // }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_from_order_with_wrong_recipient_ata(
                test_state: &mut TestState,
            ) {
                local_helpers::test_cannot_rescue_funds_from_order_with_wrong_recipient_ata(
                    test_state,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_from_order_with_wrong_order_ata(
                test_state: &mut TestState,
            ) {
                local_helpers::test_cannot_rescue_funds_from_order_with_wrong_orders_ata(
                    test_state,
                )
                .await
            }
        }

        mod test_order_rescue_funds_for_escrow {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_rescue_all_tokens_and_close_ata(test_state: &mut TestState) {
                create_order(test_state).await;
                common_escrow_tests::test_rescue_all_tokens_and_close_ata(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_rescue_part_of_tokens_and_not_close_ata(test_state: &mut TestState) {
                create_order(test_state).await;
                common_escrow_tests::test_rescue_part_of_tokens_and_not_close_ata(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_before_rescue_delay_pass(
                test_state: &mut TestState,
            ) {
                create_order(test_state).await;
                common_escrow_tests::test_cannot_rescue_funds_before_rescue_delay_pass(
                    test_state,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_by_non_recipient(test_state: &mut TestState) {
                create_order(test_state).await;
                common_escrow_tests::test_cannot_rescue_funds_by_non_recipient(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_with_wrong_recipient_ata(
                test_state: &mut TestState,
            ) {
                create_order(test_state).await;
                common_escrow_tests::test_cannot_rescue_funds_with_wrong_recipient_ata(
                    test_state,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_with_wrong_order_ata(test_state: &mut TestState) {
                create_order(test_state).await;
                common_escrow_tests::test_cannot_rescue_funds_with_wrong_escrow_ata(test_state)
                    .await
            }
        }

        mod test_order_creation_cost {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_tx_cost(test_state: &mut TestState) {
                common_escrow_tests::test_escrow_creation_tx_cost(test_state).await
            }
        }
    }
);

mod local_helpers {
    use super::*;

    use anchor_lang::InstructionData;
    use cross_chain_escrow_src::merkle_tree::MerkleProof;
    use solana_program::instruction::{AccountMeta, Instruction};
    use solana_program::pubkey::Pubkey;
    use solana_program::system_program::ID as system_program_id;
    use solana_sdk::keccak::{hashv, Hash};
    use solana_sdk::signature::Signer;
    use solana_sdk::transaction::Transaction;

    /// Byte offset in the escrow account data where the `dst_amount` field is located
    const DST_AMOUNT_OFFSET: usize = 205;
    const U64_SIZE: usize = size_of::<u64>();

    pub fn create_public_escrow_cancel_tx<S: TokenVariant>(
        test_state: &TestStateBase<SrcProgram, S>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
        canceller: &Keypair,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_src::instruction::PublicCancelEscrow {});

        let (creator_ata, _) = find_user_ata(test_state);

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_src::id(),
            accounts: vec![
                AccountMeta::new(test_state.recipient_wallet.keypair.pubkey(), false),
                AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), false),
                AccountMeta::new_readonly(test_state.token, false),
                AccountMeta::new(canceller.pubkey(), true),
                AccountMeta::new(*escrow, false),
                AccountMeta::new(*escrow_ata, false),
                AccountMeta::new(creator_ata, false),
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

    /// Reads the `dst_amount` field (u64) directly from the raw account data.
    pub fn get_dst_amount(data: &[u8]) -> Option<u64> {
        let end = DST_AMOUNT_OFFSET + U64_SIZE;
        let slice = data.get(DST_AMOUNT_OFFSET..end)?;
        let mut arr = [0u8; U64_SIZE];
        arr.copy_from_slice(slice);
        Some(u64::from_le_bytes(arr))
    }

    pub async fn test_order_creation<S: TokenVariant>(
        test_state: &mut TestStateBase<SrcProgram, S>,
    ) {
        let (order, order_ata, transaction) = create_order_data(test_state);

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_success();

        let (creator_ata, _) = find_user_ata(test_state);

        // Check token balance for the order is as expected.
        assert_eq!(
            DEFAULT_ESCROW_AMOUNT,
            get_token_balance(&mut test_state.context, &order_ata).await
        );

        // Check the lamport balance of order account is as expected.
        let order_data_len = get_order_data_len();
        let rent_lamports = get_min_rent_for_size(&mut test_state.client, order_data_len).await;

        let order_ata_lamports =
            get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;
        assert_eq!(
            rent_lamports,
            test_state.client.get_balance(order).await.unwrap()
        );

        // Check the token or lamport balance of creator account is as expected.
        if !test_state.test_arguments.asset_is_native {
            assert_eq!(
                WALLET_DEFAULT_TOKENS - DEFAULT_ESCROW_AMOUNT,
                get_token_balance(&mut test_state.context, &creator_ata).await
            );
        } else {
            // Check native balance for the creator is as expected.
            assert_eq!(
                WALLET_DEFAULT_LAMPORTS
                    - DEFAULT_ESCROW_AMOUNT
                    - order_ata_lamports
                    - rent_lamports,
                // The pure lamport balance of the creator wallet after the transaction.
                test_state
                    .client
                    .get_balance(test_state.creator_wallet.keypair.pubkey())
                    .await
                    .unwrap()
            );
        }

        // Calculate the wrapped SOL amount if the token is NATIVE_MINT to adjust the escrow ATA balance.
        let wrapped_sol = if test_state.token == NATIVE_MINT {
            test_state.test_arguments.order_amount
        } else {
            0
        };

        assert_eq!(
            order_ata_lamports,
            test_state.client.get_balance(order_ata).await.unwrap() - wrapped_sol
        );
    }

    pub async fn test_public_cancel_escrow<S: TokenVariant>(
        test_state: &mut TestStateBase<SrcProgram, S>,
        canceller: &Keypair,
    ) {
        create_order(test_state).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction =
            create_public_escrow_cancel_tx(test_state, &escrow, &escrow_ata, canceller);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp
                + DEFAULT_PERIOD_DURATION * PeriodType::PublicCancellation as u32,
        );

        let escrow_data_len = <SrcProgram as EscrowVariant<Token2022>>::get_escrow_data_len();
        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;

        let (creator_ata, _) = find_user_ata(test_state);

        let balance_changes: Vec<BalanceChange> = if canceller
            != &test_state.recipient_wallet.keypair
        {
            [
                token_change(creator_ata, DEFAULT_ESCROW_AMOUNT),
                native_change(canceller.pubkey(), test_state.test_arguments.safety_deposit),
                native_change(
                    test_state.recipient_wallet.keypair.pubkey(),
                    rent_lamports + token_account_rent - test_state.test_arguments.safety_deposit,
                ),
            ]
            .to_vec()
        } else {
            [
                token_change(creator_ata, DEFAULT_ESCROW_AMOUNT),
                native_change(
                    test_state.recipient_wallet.keypair.pubkey(),
                    rent_lamports + token_account_rent,
                ),
            ]
            .to_vec()
        };

        test_state
            .expect_balance_change(transaction, &balance_changes)
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

    fn get_rescue_funds_from_order_tx<S: TokenVariant>(
        test_state: &mut TestStateBase<SrcProgram, S>,
        order: &Pubkey,
        order_ata: &Pubkey,
        token_to_rescue: &Pubkey,
        recipient_ata: &Pubkey,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_src::instruction::RescueFundsForOrder {
                hashlock: test_state.hashlock.to_bytes(),
                order_hash: test_state.order_hash.to_bytes(),
                order_creator: test_state.creator_wallet.keypair.pubkey(),
                order_mint: test_state.token,
                order_amount: test_state.test_arguments.order_amount,
                safety_deposit: test_state.test_arguments.safety_deposit,
                rescue_start: test_state.test_arguments.rescue_start,
                rescue_amount: test_state.test_arguments.rescue_amount,
            });

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_src::id(),
            accounts: vec![
                AccountMeta::new(test_state.recipient_wallet.keypair.pubkey(), true),
                AccountMeta::new_readonly(*token_to_rescue, false),
                AccountMeta::new(*order, false),
                AccountMeta::new(*order_ata, false),
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

    pub async fn test_rescue_all_tokens_from_order_and_close_ata<S: TokenVariant>(
        test_state: &mut TestStateBase<SrcProgram, S>,
    ) {
        let (order, _) = create_order(test_state).await;

        let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
        let order_ata =
            S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &order)
                .await;
        let recipient_ata = S::initialize_spl_associated_account(
            &mut test_state.context,
            &token_to_rescue,
            &test_state.recipient_wallet.keypair.pubkey(),
        )
        .await;

        S::mint_spl_tokens(
            &mut test_state.context,
            &token_to_rescue,
            &order_ata,
            &test_state.payer_kp.pubkey(),
            &test_state.payer_kp,
            test_state.test_arguments.rescue_amount,
        )
        .await;

        let transaction = get_rescue_funds_from_order_tx(
            test_state,
            &order,
            &order_ata,
            &token_to_rescue,
            &recipient_ata,
        );

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + common::constants::RESCUE_DELAY + 100,
        );
        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(
                        test_state.recipient_wallet.keypair.pubkey(),
                        token_account_rent,
                    ),
                    token_change(recipient_ata, test_state.test_arguments.rescue_amount),
                ],
            )
            .await;

        // Assert escrow_ata was closed
        assert!(test_state
            .client
            .get_account(order_ata)
            .await
            .unwrap()
            .is_none());
    }

    pub async fn test_rescue_part_of_tokens_from_order_and_not_close_ata<S: TokenVariant>(
        test_state: &mut TestStateBase<SrcProgram, S>,
    ) {
        let (order, _) = create_order(test_state).await;

        let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
        let order_ata =
            S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &order)
                .await;
        let recipient_ata = S::initialize_spl_associated_account(
            &mut test_state.context,
            &token_to_rescue,
            &test_state.recipient_wallet.keypair.pubkey(),
        )
        .await;

        S::mint_spl_tokens(
            &mut test_state.context,
            &token_to_rescue,
            &order_ata,
            &test_state.payer_kp.pubkey(),
            &test_state.payer_kp,
            test_state.test_arguments.rescue_amount,
        )
        .await;

        // Rescue only half of tokens from order ata.
        test_state.test_arguments.rescue_amount /= 2;
        let transaction = get_rescue_funds_from_order_tx(
            test_state,
            &order,
            &order_ata,
            &token_to_rescue,
            &recipient_ata,
        );

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + common::constants::RESCUE_DELAY + 100,
        );

        test_state
            .expect_balance_change(
                transaction,
                &[token_change(
                    recipient_ata,
                    test_state.test_arguments.rescue_amount,
                )],
            )
            .await;

        // Assert order_ata was not closed
        assert!(test_state
            .client
            .get_account(order_ata)
            .await
            .unwrap()
            .is_some());
    }

    pub async fn test_cannot_rescue_funds_from_order_before_rescue_delay_pass<S: TokenVariant>(
        test_state: &mut TestStateBase<SrcProgram, S>,
    ) {
        let (order, _) = create_order(test_state).await;

        let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
        let order_ata =
            S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &order)
                .await;
        let recipient_ata = S::initialize_spl_associated_account(
            &mut test_state.context,
            &token_to_rescue,
            &test_state.recipient_wallet.keypair.pubkey(),
        )
        .await;

        S::mint_spl_tokens(
            &mut test_state.context,
            &token_to_rescue,
            &order_ata,
            &test_state.payer_kp.pubkey(),
            &test_state.payer_kp,
            test_state.test_arguments.rescue_amount,
        )
        .await;

        let transaction = get_rescue_funds_from_order_tx(
            test_state,
            &order,
            &order_ata,
            &token_to_rescue,
            &recipient_ata,
        );

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + common::constants::RESCUE_DELAY - 100,
        );

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((0, ProgramError::Custom(EscrowError::InvalidTime.into())));
    }

    pub async fn _test_cannot_rescue_funds_from_order_by_non_recipient<S: TokenVariant>(
        // TODO: use after implement whitelist
        test_state: &mut TestStateBase<SrcProgram, S>,
    ) {
        let (order, _) = create_order(test_state).await;

        let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
        let order_ata =
            S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &order)
                .await;
        test_state.recipient_wallet = test_state.creator_wallet.clone(); // Use different wallet as recipient
        let recipient_ata = S::initialize_spl_associated_account(
            &mut test_state.context,
            &token_to_rescue,
            &test_state.recipient_wallet.keypair.pubkey(),
        )
        .await;

        S::mint_spl_tokens(
            &mut test_state.context,
            &token_to_rescue,
            &order_ata,
            &test_state.payer_kp.pubkey(),
            &test_state.payer_kp,
            test_state.test_arguments.rescue_amount,
        )
        .await;

        let transaction = get_rescue_funds_from_order_tx(
            test_state,
            &order,
            &order_ata,
            &token_to_rescue,
            &recipient_ata,
        );

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + common::constants::RESCUE_DELAY + 100,
        );

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((0, ProgramError::Custom(ErrorCode::ConstraintSeeds.into())))
    }

    pub async fn test_cannot_rescue_funds_from_order_with_wrong_recipient_ata<S: TokenVariant>(
        test_state: &mut TestStateBase<SrcProgram, S>,
    ) {
        let (order, _) = create_order(test_state).await;

        let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
        let order_ata =
            S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &order)
                .await;

        S::mint_spl_tokens(
            &mut test_state.context,
            &token_to_rescue,
            &order_ata,
            &test_state.payer_kp.pubkey(),
            &test_state.payer_kp,
            test_state.test_arguments.rescue_amount,
        )
        .await;

        let wrong_recipient_ata = S::initialize_spl_associated_account(
            &mut test_state.context,
            &token_to_rescue,
            &test_state.creator_wallet.keypair.pubkey(),
        )
        .await;

        let transaction = get_rescue_funds_from_order_tx(
            test_state,
            &order,
            &order_ata,
            &token_to_rescue,
            &wrong_recipient_ata,
        );

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + common::constants::RESCUE_DELAY + 100,
        );

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                0,
                ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()),
            ))
    }

    pub async fn test_cannot_rescue_funds_from_order_with_wrong_orders_ata<S: TokenVariant>(
        test_state: &mut TestStateBase<SrcProgram, S>,
    ) {
        let (order, order_ata) = create_order(test_state).await;

        let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
        let recipient_ata = S::initialize_spl_associated_account(
            &mut test_state.context,
            &token_to_rescue,
            &test_state.recipient_wallet.keypair.pubkey(),
        )
        .await;

        let transaction = get_rescue_funds_from_order_tx(
            test_state,
            &order,
            &order_ata, // Use order ata for order mint, but not for token to rescue
            &token_to_rescue,
            &recipient_ata,
        );

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + common::constants::RESCUE_DELAY + 100,
        );

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                0,
                ProgramError::Custom(ErrorCode::ConstraintAssociated.into()),
            ))
    }

    pub async fn test_order_cancel<S: TokenVariant>(test_state: &mut TestStateBase<SrcProgram, S>) {
        let (order, order_ata) = create_order(test_state).await;
        let transaction = get_cancel_order_tx(test_state, &order, &order_ata, None);

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;
        let order_rent = get_min_rent_for_size(&mut test_state.client, get_order_data_len()).await;

        let (creator_ata, _) = find_user_ata(test_state);

        let balance_changes: Vec<BalanceChange> = if test_state.test_arguments.asset_is_native {
            vec![native_change(
                test_state.creator_wallet.keypair.pubkey(),
                token_account_rent + order_rent + test_state.test_arguments.order_amount,
            )]
        } else {
            vec![
                token_change(creator_ata, test_state.test_arguments.order_amount),
                native_change(
                    test_state.creator_wallet.keypair.pubkey(),
                    token_account_rent + order_rent,
                ),
            ]
        };

        test_state
            .expect_balance_change(transaction, &balance_changes)
            .await;

        let acc_lookup_result = test_state.client.get_account(order).await.unwrap();
        assert!(acc_lookup_result.is_none());

        let acc_lookup_result = test_state.client.get_account(order_ata).await.unwrap();
        assert!(acc_lookup_result.is_none());
    }

    pub async fn test_cancel_by_resolver_for_free_at_the_auction_start<S: TokenVariant>(
        test_state: &mut TestStateBase<SrcProgram, S>,
    ) {
        let (order, order_ata) = create_order(test_state).await;
        let transaction = get_cancel_order_by_resolver_tx(test_state, &order, &order_ata, None);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + test_state.test_arguments.expiration_duration,
        );

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;

        let order_rent = get_min_rent_for_size(&mut test_state.client, get_order_data_len()).await;

        let (creator_ata, _) = find_user_ata(test_state);

        let balance_changes: Vec<BalanceChange> = if test_state.test_arguments.asset_is_native {
            vec![native_change(
                test_state.creator_wallet.keypair.pubkey(),
                token_account_rent + order_rent + test_state.test_arguments.order_amount,
            )]
        } else {
            vec![
                token_change(creator_ata, test_state.test_arguments.order_amount),
                native_change(
                    test_state.creator_wallet.keypair.pubkey(),
                    token_account_rent + order_rent,
                ),
            ]
        };

        test_state
            .expect_balance_change(transaction, &balance_changes)
            .await;

        let acc_lookup_result = test_state.client.get_account(order).await.unwrap();
        assert!(acc_lookup_result.is_none());

        let acc_lookup_result = test_state.client.get_account(order_ata).await.unwrap();
        assert!(acc_lookup_result.is_none());
    }

    pub async fn test_cancel_by_resolver_at_different_points<S: TokenVariant>(
        init_test_state: &mut TestStateBase<SrcProgram, S>,
        asset_is_native: bool,
        native_mint: Option<Pubkey>,
    ) {
        let token_account_rent = get_min_rent_for_size(
            &mut init_test_state.client,
            local_helpers::get_token_account_len(PhantomData::<TestState>),
        )
        .await;

        let cancellation_points: Vec<u32> = vec![10, 25, 50, 100]
            .into_iter()
            .map(|percentage| {
                (init_test_state.test_arguments.expiration_duration
                    + init_test_state.init_timestamp)
                    + (init_test_state.test_arguments.cancellation_auction_duration
                        * (percentage * 100))
                        / (100 * 100)
            })
            .collect();

        for &cancellation_point in &cancellation_points {
            let max_cancellation_premiums: Vec<f64> = vec![1.0, 2.5, 7.5]
                .into_iter()
                .map(|percentage| {
                    (token_account_rent as f64 * (percentage * 100_f64)) / (100_f64 * 100_f64)
                })
                .collect();

            for &max_cancellation_premium in &max_cancellation_premiums {
                // Create a new test state for each cancellation point and premium
                let mut test_state =
                    local_helpers::reset_test_state(PhantomData::<TestState>).await;

                // Set max cancellation premium
                test_state.test_arguments.max_cancellation_premium =
                    max_cancellation_premium as u64;

                // Ensure reward limit is equal to max cancellation premium
                test_state.test_arguments.reward_limit = max_cancellation_premium as u64;
                if native_mint.is_some() {
                    test_state.token = native_mint.unwrap();
                }
                test_state.test_arguments.asset_is_native = asset_is_native;

                let (order, order_ata) = create_order(&test_state).await;
                let transaction =
                    get_cancel_order_by_resolver_tx(&test_state, &order, &order_ata, None);

                set_time(&mut test_state.context, cancellation_point);

                let expiratione_time =
                    test_state.test_arguments.expiration_duration + test_state.init_timestamp;

                let order_rent =
                    get_min_rent_for_size(&mut test_state.client, get_order_data_len()).await;

                let clock: Clock = test_state
                    .client
                    .get_sysvar::<Clock>()
                    .await
                    .expect("Failed to get Clock sysvar");

                let resolver_premium = calculate_premium(
                    clock.unix_timestamp as u32,
                    expiratione_time,
                    test_state.test_arguments.cancellation_auction_duration,
                    max_cancellation_premium as u64,
                );

                let (creator_ata, _) = find_user_ata(&test_state);

                let balance_changes: Vec<BalanceChange> =
                    if test_state.test_arguments.asset_is_native {
                        vec![
                            native_change(
                                test_state.creator_wallet.keypair.pubkey(),
                                token_account_rent + order_rent - resolver_premium
                                    + test_state.test_arguments.order_amount,
                            ),
                            native_change(
                                test_state.recipient_wallet.keypair.pubkey(),
                                resolver_premium,
                            ),
                        ]
                    } else {
                        vec![
                            token_change(creator_ata, test_state.test_arguments.order_amount),
                            native_change(
                                test_state.creator_wallet.keypair.pubkey(),
                                token_account_rent + order_rent - resolver_premium,
                            ),
                            native_change(
                                test_state.recipient_wallet.keypair.pubkey(),
                                resolver_premium,
                            ),
                        ]
                    };

                test_state
                    .expect_balance_change(transaction, &balance_changes)
                    .await;

                let order_acc = test_state.client.get_account(order).await.unwrap();
                assert!(order_acc.is_none());

                let ata_acc = test_state.client.get_account(order_ata).await.unwrap();
                assert!(ata_acc.is_none());
            }
        }
    }

    pub async fn test_cancel_by_resolver_after_auction<S: TokenVariant>(
        test_state: &mut TestStateBase<SrcProgram, S>,
    ) {
        let (order, order_ata) = create_order(test_state).await;

        let transaction = get_cancel_order_by_resolver_tx(test_state, &order, &order_ata, None);

        let expiratione_time =
            test_state.test_arguments.expiration_duration + test_state.init_timestamp;

        set_time(
            &mut test_state.context,
            expiratione_time + test_state.test_arguments.cancellation_auction_duration + 1,
        );

        let resolver_premium = test_state.test_arguments.max_cancellation_premium;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;

        let order_rent = get_min_rent_for_size(&mut test_state.client, get_order_data_len()).await;

        let (creator_ata, _) = find_user_ata(test_state);

        let balance_changes: Vec<BalanceChange> = if test_state.test_arguments.asset_is_native {
            vec![
                native_change(
                    test_state.creator_wallet.keypair.pubkey(),
                    token_account_rent + order_rent - resolver_premium
                        + test_state.test_arguments.order_amount,
                ),
                native_change(
                    test_state.recipient_wallet.keypair.pubkey(),
                    resolver_premium,
                ),
            ]
        } else {
            vec![
                token_change(creator_ata, test_state.test_arguments.order_amount),
                native_change(
                    test_state.creator_wallet.keypair.pubkey(),
                    token_account_rent + order_rent - resolver_premium,
                ),
                native_change(
                    test_state.recipient_wallet.keypair.pubkey(),
                    resolver_premium,
                ),
            ]
        };

        test_state
            .expect_balance_change(transaction, &balance_changes)
            .await;
    }

    pub async fn test_cancel_by_resolver_reward_less_then_auction_calculated<S: TokenVariant>(
        test_state: &mut TestStateBase<SrcProgram, S>,
    ) {
        let (order, order_ata) = create_order(test_state).await;

        let resolver_premium: u64 = 1;

        test_state.test_arguments.reward_limit = resolver_premium;

        let transaction = get_cancel_order_by_resolver_tx(test_state, &order, &order_ata, None);

        let expiratione_time =
            test_state.test_arguments.expiration_duration + test_state.init_timestamp;

        set_time(
            &mut test_state.context,
            expiratione_time + test_state.test_arguments.cancellation_auction_duration + 1,
        );

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;

        let order_rent = get_min_rent_for_size(&mut test_state.client, get_order_data_len()).await;

        let (creator_ata, _) = find_user_ata(test_state);

        let balance_changes: Vec<BalanceChange> = if test_state.test_arguments.asset_is_native {
            vec![
                native_change(
                    test_state.creator_wallet.keypair.pubkey(),
                    token_account_rent + order_rent - resolver_premium
                        + test_state.test_arguments.order_amount,
                ),
                native_change(
                    test_state.recipient_wallet.keypair.pubkey(),
                    resolver_premium,
                ),
            ]
        } else {
            vec![
                token_change(creator_ata, test_state.test_arguments.order_amount),
                native_change(
                    test_state.creator_wallet.keypair.pubkey(),
                    token_account_rent + order_rent - resolver_premium,
                ),
                native_change(
                    test_state.recipient_wallet.keypair.pubkey(),
                    resolver_premium,
                ),
            ]
        };

        test_state
            .expect_balance_change(transaction, &balance_changes)
            .await;
    }

    pub async fn create_order_for_partial_fill(test_state: &mut TestState) -> (Pubkey, Pubkey) {
        test_state.test_arguments.order_parts_amount = DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
        let merkle_hashes = compute_merkle_leaves(test_state);
        let root = get_root(merkle_hashes.leaves.clone());
        test_state.hashlock = Hash::new_from_array(root);
        test_state.test_arguments.allow_multiple_fills = true;
        create_order(test_state).await
    }

    pub async fn create_escrow_for_partial_fill_data(
        test_state: &mut TestState,
        escrow_amount: u64,
    ) -> (Pubkey, Pubkey, Transaction) {
        let merkle_hashes = compute_merkle_leaves(test_state);
        let index_to_validate = get_index_for_escrow_amount(test_state, escrow_amount);
        test_state.test_arguments.escrow_amount = escrow_amount;

        let hashed_secret = merkle_hashes.hashed_secrets[index_to_validate];
        let proof_hashes = get_proof(merkle_hashes.leaves.clone(), index_to_validate);
        let proof = MerkleProof {
            proof: proof_hashes,
            index: index_to_validate as u32,
            hashed_secret,
        };

        test_state.test_arguments.merkle_proof = Some(proof);
        create_escrow_data(test_state)
    }

    pub async fn create_escrow_for_partial_fill(
        test_state: &mut TestState,
        escrow_amount: u64,
    ) -> (Pubkey, Pubkey) {
        let (escrow, escrow_ata, transaction) =
            create_escrow_for_partial_fill_data(test_state, escrow_amount).await;

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_success();

        test_state.test_arguments.order_remaining_amount -= escrow_amount;
        (escrow, escrow_ata)
    }

    use std::marker::PhantomData;

    pub fn get_token_account_len<T, S: TokenVariant>(_: PhantomData<TestStateBase<T, S>>) -> usize {
        S::get_token_account_size()
    }

    pub async fn reset_test_state<T, S: TokenVariant>(
        _: PhantomData<TestStateBase<T, S>>,
    ) -> TestStateBase<SrcProgram, S> {
        <TestStateBase<SrcProgram, S> as test_context::AsyncTestContext>::setup().await
    }

    pub struct MerkleHashes {
        pub leaves: Vec<[u8; 32]>,
        pub hashed_secrets: Vec<[u8; 32]>,
    }

    pub fn compute_merkle_leaves<T: EscrowVariant<S>, S: TokenVariant>(
        test_state: &TestStateBase<T, S>,
    ) -> MerkleHashes {
        let secret_amount = (test_state.test_arguments.order_parts_amount + 1) as usize;
        let mut leaves = Vec::with_capacity(secret_amount);
        let mut hashed_secrets = Vec::with_capacity(secret_amount);

        for i in 0..secret_amount {
            let i_bytes = (i as u64).to_be_bytes();
            let hashed_bytes = hashv(&[&i_bytes]).0;
            let hashed_secret = hashv(&[&hashed_bytes]).0;
            hashed_secrets.push(hashed_secret);

            let pair_data = [&i_bytes[..], &hashed_secret[..]];
            let hashed_pair = hashv(&pair_data).0;
            leaves.push(hashed_pair);
        }

        MerkleHashes {
            leaves,
            hashed_secrets,
        }
    }

    pub fn get_index_for_escrow_amount<T: EscrowVariant<S>, S: TokenVariant>(
        test_state: &TestStateBase<T, S>,
        escrow_amount: u64,
    ) -> usize {
        if escrow_amount == test_state.test_arguments.order_remaining_amount {
            return test_state.test_arguments.order_parts_amount as usize;
        }
        ((test_state.test_arguments.order_amount
            - test_state.test_arguments.order_remaining_amount
            + escrow_amount
            - 1)
            * test_state.test_arguments.order_parts_amount
            / test_state.test_arguments.order_amount) as usize
    }
}

// Native Mint (wrapped SOL) is always owned by the SPL Token program
type TestState = TestStateBase<SrcProgram, TokenSPL>;

mod test_native_src {
    use solana_program_pack::Pack;

    use super::*;
    use crate::local_helpers::create_public_escrow_cancel_tx;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_order_creation(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        local_helpers::test_order_creation(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;
        common_escrow_tests::test_escrow_creation_native(
            test_state,
            test_state.recipient_wallet.keypair.pubkey(),
        )
        .await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_order_creation_fails_if_token_is_not_native(test_state: &mut TestState) {
        test_state.test_arguments.asset_is_native = true;
        let (_, _, tx) = create_order_data(test_state);
        test_state
            .client
            .process_transaction(tx)
            .await
            .expect_error((
                0,
                ProgramError::Custom(EscrowError::InconsistentNativeTrait.into()),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;
        let rent_recipient = test_state.recipient_wallet.keypair.pubkey();
        common_escrow_tests::test_withdraw(test_state, rent_recipient).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_by_resolver(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;
        let withdrawer = test_state.recipient_wallet.keypair.insecure_clone();
        let rent_recipient = test_state.recipient_wallet.keypair.pubkey();
        common_escrow_tests::test_public_withdraw_tokens(test_state, withdrawer, rent_recipient)
            .await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_by_any_account(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;
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
        let rent_recipient = test_state.recipient_wallet.keypair.pubkey();
        common_escrow_tests::test_public_withdraw_tokens(test_state, withdrawer, rent_recipient)
            .await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_order_cancel(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        local_helpers::test_order_cancel(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_order_cancel_fails_if_maker_ata_is_provided(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        let (order, order_ata) = create_order(test_state).await;
        let transaction = get_cancel_order_tx(
            test_state,
            &order,
            &order_ata,
            Some(&test_state.creator_wallet.native_token_account),
        );

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
        );

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                0,
                ProgramError::Custom(EscrowError::InconsistentNativeTrait.into()),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_for_free_at_the_auction_start(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        local_helpers::test_cancel_by_resolver_for_free_at_the_auction_start(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_at_different_points(test_state: &mut TestState) {
        local_helpers::test_cancel_by_resolver_at_different_points(
            test_state,
            true,
            Some(NATIVE_MINT),
        )
        .await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_after_auction(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        local_helpers::test_cancel_by_resolver_after_auction(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_reward_less_then_auction_calculated(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        local_helpers::test_cancel_by_resolver_reward_less_then_auction_calculated(test_state)
            .await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_fails_if_native_and_maker_ata_provided(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        let (order, order_ata) = create_order(test_state).await;

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + test_state.test_arguments.expiration_duration,
        );

        let transaction = get_cancel_order_by_resolver_tx(
            test_state,
            &order,
            &order_ata,
            Some(&test_state.creator_wallet.native_token_account),
        );

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                0,
                ProgramError::Custom(EscrowError::InconsistentNativeTrait.into()),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;
        common_escrow_tests::test_cancel_native(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_cancel_by_any_account(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let canceller = Keypair::new();
        transfer_lamports(
            &mut test_state.context,
            WALLET_DEFAULT_LAMPORTS,
            &test_state.payer_kp,
            &canceller.pubkey(),
        )
        .await;

        let transaction =
            create_public_escrow_cancel_tx(test_state, &escrow, &escrow_ata, &canceller);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp
                + DEFAULT_PERIOD_DURATION * PeriodType::PublicCancellation as u32,
        );

        let escrow_data_len = <SrcProgram as EscrowVariant<Token2022>>::get_escrow_data_len();
        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, SplTokenAccount::LEN).await;

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(canceller.pubkey(), test_state.test_arguments.safety_deposit),
                    native_change(
                        test_state.creator_wallet.keypair.pubkey(),
                        test_state.test_arguments.escrow_amount,
                    ),
                    native_change(
                        test_state.recipient_wallet.keypair.pubkey(),
                        rent_lamports + token_account_rent
                            - test_state.test_arguments.safety_deposit,
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
    async fn test_rescue_all_tokens_and_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;
        common_escrow_tests::test_rescue_all_tokens_and_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_part_of_tokens_and_not_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;
        common_escrow_tests::test_rescue_part_of_tokens_and_not_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_all_tokens_from_order_and_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        local_helpers::test_rescue_all_tokens_from_order_and_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_part_of_tokens_from_order_and_not_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        local_helpers::test_rescue_part_of_tokens_from_order_and_not_close_ata(test_state).await
    }
}

mod test_wrapped_native {
    use solana_program_pack::Pack;

    use super::*;
    use crate::local_helpers::create_public_escrow_cancel_tx;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_order_creation(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        local_helpers::test_order_creation(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order(test_state).await;
        common_escrow_tests::test_escrow_creation(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order(test_state).await;
        let rent_recipient = test_state.recipient_wallet.keypair.pubkey();
        common_escrow_tests::test_withdraw(test_state, rent_recipient).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_by_resolver(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order(test_state).await;
        let withdrawer = test_state.recipient_wallet.keypair.insecure_clone();
        let rent_recipient = test_state.recipient_wallet.keypair.pubkey();
        common_escrow_tests::test_public_withdraw_tokens(test_state, withdrawer, rent_recipient)
            .await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_by_any_account(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order(test_state).await;
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
        let rent_recipient = test_state.recipient_wallet.keypair.pubkey();
        common_escrow_tests::test_public_withdraw_tokens(test_state, withdrawer, rent_recipient)
            .await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_order_cancel(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        local_helpers::test_order_cancel(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_order_cancel_fails_if_maker_ata_is_not_provided(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        let (order, order_ata) = create_order(test_state).await;
        let transaction = get_cancel_order_tx(
            test_state,
            &order,
            &order_ata,
            Some(&cross_chain_escrow_src::id()),
        );

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
        );

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                0,
                ProgramError::Custom(EscrowError::InconsistentNativeTrait.into()),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_for_free_at_the_auction_start(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        local_helpers::test_cancel_by_resolver_for_free_at_the_auction_start(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_at_different_points(test_state: &mut TestState) {
        local_helpers::test_cancel_by_resolver_at_different_points(
            test_state,
            false,
            Some(NATIVE_MINT),
        )
        .await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_after_auction(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        local_helpers::test_cancel_by_resolver_after_auction(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_reward_less_then_auction_calculated(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        local_helpers::test_cancel_by_resolver_reward_less_then_auction_calculated(test_state)
            .await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_fails_if_wrapped_and_maker_ata_not_provided(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        let (order, order_ata) = create_order(test_state).await;

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + test_state.test_arguments.expiration_duration,
        );

        let transaction = get_cancel_order_by_resolver_tx(
            test_state,
            &order,
            &order_ata,
            Some(&cross_chain_escrow_src::id()),
        );

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                0,
                ProgramError::Custom(EscrowError::InconsistentNativeTrait.into()),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order(test_state).await;
        common_escrow_tests::test_cancel(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_cancel_by_any_account(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;

        create_order(test_state).await;

        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let canceller = Keypair::new();
        transfer_lamports(
            &mut test_state.context,
            WALLET_DEFAULT_LAMPORTS,
            &test_state.payer_kp,
            &canceller.pubkey(),
        )
        .await;

        let transaction =
            create_public_escrow_cancel_tx(test_state, &escrow, &escrow_ata, &canceller);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp
                + DEFAULT_PERIOD_DURATION * PeriodType::PublicCancellation as u32,
        );

        let escrow_data_len = <SrcProgram as EscrowVariant<Token2022>>::get_escrow_data_len();
        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, SplTokenAccount::LEN).await;

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(canceller.pubkey(), test_state.test_arguments.safety_deposit),
                    token_change(
                        test_state.creator_wallet.native_token_account,
                        test_state.test_arguments.escrow_amount,
                    ),
                    native_change(
                        test_state.recipient_wallet.keypair.pubkey(),
                        rent_lamports + token_account_rent
                            - test_state.test_arguments.safety_deposit,
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
    async fn test_rescue_all_tokens_and_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order(test_state).await;
        common_escrow_tests::test_rescue_all_tokens_and_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_part_of_tokens_and_not_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order(test_state).await;
        common_escrow_tests::test_rescue_part_of_tokens_and_not_close_ata(test_state).await
    }
}

mod test_partial_fill_escrow_creation {
    use crate::local_helpers::{
        compute_merkle_leaves, create_escrow_for_partial_fill, create_escrow_for_partial_fill_data,
        create_order_for_partial_fill, get_index_for_escrow_amount,
    };

    use super::*;
    use cross_chain_escrow_src::merkle_tree::MerkleProof;
    use solana_sdk::keccak::{hashv, Hash};

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_create_escrow_with_merkle_proof_and_leaf_validation(test_state: &mut TestState) {
        let (order, order_ata) = create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
        create_escrow_for_partial_fill(test_state, escrow_amount).await;

        // Check that the order accounts have not been closed.
        let acc_lookup_result = test_state.client.get_account(order).await.unwrap();
        assert!(acc_lookup_result.is_some());

        let acc_lookup_result = test_state.client.get_account(order_ata).await.unwrap();
        assert!(acc_lookup_result.is_some());
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_create_two_escrows_for_separate_parts(test_state: &mut TestState) {
        let (order, order_ata) = create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
        create_escrow_for_partial_fill(test_state, escrow_amount).await;
        create_escrow_for_partial_fill(test_state, escrow_amount).await;

        // Check that the order accounts have not been closed.
        let acc_lookup_result = test_state.client.get_account(order).await.unwrap();
        assert!(acc_lookup_result.is_some());

        let acc_lookup_result = test_state.client.get_account(order_ata).await.unwrap();
        assert!(acc_lookup_result.is_some());
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_create_escrow_with_merkle_proof_and_leaf_validation_for_full_fill(
        test_state: &mut TestState,
    ) {
        let (order, order_ata) = create_order_for_partial_fill(test_state).await;

        create_escrow_for_partial_fill(test_state, DEFAULT_ESCROW_AMOUNT).await;

        // Check that the order accounts have been closed.
        let acc_lookup_result = test_state.client.get_account(order).await.unwrap();
        assert!(acc_lookup_result.is_none());

        let acc_lookup_result = test_state.client.get_account(order_ata).await.unwrap();
        assert!(acc_lookup_result.is_none());
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_create_two_escrows_for_full_order(test_state: &mut TestState) {
        let (order, order_ata) = create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
        create_escrow_for_partial_fill(test_state, escrow_amount).await;
        create_escrow_for_partial_fill(
            test_state,
            DEFAULT_ESCROW_AMOUNT - escrow_amount, // full fill
        )
        .await;

        // Check that the order accounts have been closed.
        let acc_lookup_result = test_state.client.get_account(order).await.unwrap();
        assert!(acc_lookup_result.is_none());

        let acc_lookup_result = test_state.client.get_account(order_ata).await.unwrap();
        assert!(acc_lookup_result.is_none());
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_create_escrow_fails_if_second_escrow_amount_too_large(
        test_state: &mut TestState,
    ) {
        create_order_for_partial_fill(test_state).await;
        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
        create_escrow_for_partial_fill(test_state, escrow_amount).await;
        let (_, _, transaction) = create_escrow_for_partial_fill_data(
            test_state,
            DEFAULT_ESCROW_AMOUNT - escrow_amount + 1,
        )
        .await;

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((0, ProgramError::Custom(EscrowError::InvalidAmount.into())));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_create_escrow_fails_if_second_escrow_have_same_proof_index(
        test_state: &mut TestState,
    ) {
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE + 1;
        create_escrow_for_partial_fill(test_state, escrow_amount).await;
        let so_small_escrow_amount = 1;
        let (_, _, transaction) =
            create_escrow_for_partial_fill_data(test_state, so_small_escrow_amount).await;

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                0,
                ProgramError::Custom(EscrowError::InvalidPartialFill.into()),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_create_escrow_fails_with_incorrect_merkle_root(test_state: &mut TestState) {
        test_state.test_arguments.order_parts_amount = DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
        let merkle_hashes = compute_merkle_leaves(test_state);
        test_state.hashlock = hashv(&[b"incorrect_root"]);
        test_state.test_arguments.allow_multiple_fills = true;
        create_order(test_state).await;

        test_state.test_arguments.escrow_amount =
            DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
        let index_to_validate =
            get_index_for_escrow_amount(test_state, test_state.test_arguments.escrow_amount);
        let hashed_secret = merkle_hashes.hashed_secrets[index_to_validate];
        let proof_hashes = get_proof(merkle_hashes.leaves.clone(), index_to_validate);
        let proof = MerkleProof {
            proof: proof_hashes,
            index: index_to_validate as u32,
            hashed_secret,
        };
        test_state.test_arguments.merkle_proof = Some(proof);
        let (_, _, transaction) = create_escrow_data(test_state);

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                0,
                ProgramError::Custom(EscrowError::InvalidMerkleProof.into()),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_create_escrow_fails_with_incorrect_merkle_proof(test_state: &mut TestState) {
        create_order_for_partial_fill(test_state).await;

        let merkle_hashes = compute_merkle_leaves(test_state);
        test_state.test_arguments.escrow_amount =
            DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
        let index_to_validate =
            get_index_for_escrow_amount(test_state, test_state.test_arguments.escrow_amount);
        let hashed_secret = merkle_hashes.hashed_secrets[index_to_validate];

        let incorrect_proof_hashes = get_proof(merkle_hashes.leaves.clone(), index_to_validate + 1);

        let proof = MerkleProof {
            proof: incorrect_proof_hashes,
            index: index_to_validate as u32,
            hashed_secret,
        };
        test_state.test_arguments.merkle_proof = Some(proof);
        let (_, _, transaction) = create_escrow_data(test_state);

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                0,
                ProgramError::Custom(EscrowError::InvalidMerkleProof.into()),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_create_escrow_fails_with_incorrect_secret_for_leaf(test_state: &mut TestState) {
        create_order_for_partial_fill(test_state).await;

        let merkle_hashes = compute_merkle_leaves(test_state);
        test_state.test_arguments.escrow_amount =
            DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 2;
        let index_to_validate =
            get_index_for_escrow_amount(test_state, test_state.test_arguments.escrow_amount);
        let proof_hashes = get_proof(merkle_hashes.leaves.clone(), index_to_validate);

        // Incorrect hashed_secret
        let proof = MerkleProof {
            proof: proof_hashes,
            index: index_to_validate as u32,
            hashed_secret: merkle_hashes.hashed_secrets[index_to_validate + 1],
        };
        test_state.test_arguments.merkle_proof = Some(proof);
        let (_, _, transaction) = create_escrow_data(test_state);

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                0,
                ProgramError::Custom(EscrowError::InvalidMerkleProof.into()),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_create_escrow_fails_without_merkle_proof(test_state: &mut TestState) {
        create_order_for_partial_fill(test_state).await;

        // test_state.test_arguments.merkle_proof is none
        let (_, _, transaction) = create_escrow_data(test_state);

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                0,
                ProgramError::Custom(EscrowError::InconsistentMerkleProofTrait.into()),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_create_escrow_fails_if_multiple_fills_are_false_and_merkle_proof_is_provided(
        test_state: &mut TestState,
    ) {
        test_state.test_arguments.order_parts_amount = DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
        let merkle_hashes = compute_merkle_leaves(test_state);
        test_state.test_arguments.order_parts_amount = DEFAULT_PARTS_AMOUNT;
        let root = get_root(merkle_hashes.leaves.clone());
        test_state.hashlock = Hash::new_from_array(root);
        // test_state.test_arguments.allow_multiple_fills is false;
        create_order(test_state).await;

        let index_to_validate =
            get_index_for_escrow_amount(test_state, test_state.test_arguments.escrow_amount); // fill the full order
        let hashed_secret = merkle_hashes.hashed_secrets[index_to_validate];
        let proof_hashes = get_proof(merkle_hashes.leaves.clone(), index_to_validate);
        let proof = MerkleProof {
            proof: proof_hashes,
            index: index_to_validate as u32,
            hashed_secret,
        };
        test_state.test_arguments.merkle_proof = Some(proof);
        let (_, _, transaction) = create_escrow_data(test_state);

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                0,
                ProgramError::Custom(EscrowError::InconsistentMerkleProofTrait.into()),
            ));
    }
}
