use anchor_lang::{error::ErrorCode, prelude::ProgramError};
use anchor_spl::token::spl_token::native_mint::ID as NATIVE_MINT;
use anchor_spl::token::spl_token::state::Account as SplTokenAccount;
use common::error::EscrowError;
use common_tests::helpers::*;
use common_tests::run_for_tokens;
use common_tests::src_program::{
    create_order, create_order_data, get_cancel_order_by_resolver_tx, get_cancel_order_tx,
    get_create_order_tx, get_order_addresses, SrcProgram,
};
use common_tests::tests as common_escrow_tests;
use common_tests::whitelist::prepare_resolvers;
use solana_program_test::tokio;
use solana_sdk::signature::Signer;
use solana_sdk::signer::keypair::Keypair;
use std::marker::PhantomData;
use test_context::test_context;

use primitive_types::U256;

pub mod helpers_src;
use helpers_src::*;

use helpers_src::merkle_tree_helpers::{get_proof, get_root};

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
                helpers_src::test_order_creation(test_state).await;
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
                    transaction.sign(&[&test_state.maker_wallet.keypair], new_hash);
                }
                if transaction.signatures.len() == 2 {
                    transaction.sign(
                        &[&test_state.maker_wallet.keypair, &test_state.context.payer],
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
                    helpers_src::get_token_account_len(std::marker::PhantomData::<TestState>),
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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_escrow_creation(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_with_excess_tokens(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                helpers_src::test_escrow_creation_with_excess_tokens(test_state).await;
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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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

                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (escrow, _) = create_escrow(test_state).await;
                let escrow_account_data = test_state
                    .client
                    .get_account(escrow)
                    .await
                    .unwrap()
                    .unwrap()
                    .data;
                let dst_amount = helpers_src::get_dst_amount(&escrow_account_data)
                    .expect("Failed to read dst_amount from escrow account data");

                let expected = U256(test_state.test_arguments.dst_amount)
                    .checked_mul(U256::from(EXPECTED_MULTIPLIER_NUMERATOR))
                    .unwrap()
                    .checked_div(U256::from(EXPECTED_MULTIPLIER_DENOMINATOR))
                    .unwrap();
                assert_eq!(U256(dst_amount), expected);
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_with_empty_order_account(
                test_state: &mut TestState,
            ) {
                // Create an escrow account without existing order account.
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_escrow_creation_fails_if_withdrawal_duration_overflows(
                    test_state,
                )
                .await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_if_public_withdrawal_duration_overflows(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.public_withdrawal_duration = u32::MAX;
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_escrow_creation_fails_if_public_withdrawal_duration_overflows(
                    test_state,
                ).await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_if_cancellation_duration_overflows(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.cancellation_duration = u32::MAX;
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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

                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                test_state.test_arguments.escrow_amount =
                    test_state.test_arguments.order_amount + 1;
                let (_, _, transaction) = create_escrow_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((0, ProgramError::Custom(EscrowError::InvalidAmount.into())));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_witout_resolver_access(test_state: &mut TestState) {
                create_order(test_state).await;
                // test_state.taker_wallet does not have resolver access
                let (_, _, transaction) = create_escrow_data(test_state);
                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((
                        0,
                        ProgramError::Custom(ErrorCode::AccountNotInitialized.into()),
                    ));
            }
        }

        mod test_escrow_withdraw {
            use super::*;
            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let rent_recipient = test_state.taker_wallet.keypair.pubkey();
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                let transaction = SrcProgram::get_withdraw_tx(test_state, &escrow, &escrow_ata);

                let token_account_rent = get_min_rent_for_size(
                    &mut test_state.client,
                    helpers_src::get_token_account_len(PhantomData::<TestState>),
                )
                .await;

                let escrow_rent =
                    get_min_rent_for_size(&mut test_state.client, DEFAULT_SRC_ESCROW_SIZE).await;

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
                );

                let (_, taker_ata) = find_user_ata(test_state);

                test_state
                    .expect_balance_change(
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
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                let transaction = SrcProgram::get_withdraw_tx(test_state, &escrow, &escrow_ata);

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
                );

                let (_, taker_ata) = find_user_ata(test_state);

                let excess_amount = 1000;
                // Send excess tokens to the escrow account
                helpers_src::mint_excess_tokens(test_state, &escrow_ata, excess_amount).await;
                test_state
                    .expect_balance_change(
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
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_withdraw_does_not_work_with_wrong_secret(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_does_not_work_with_non_recipient(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_withdraw_does_not_work_with_non_recipient(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_does_not_work_with_wrong_taker_ata(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_withdraw_does_not_work_with_wrong_taker_ata(test_state)
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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_withdraw_does_not_work_before_withdrawal_start(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_does_not_work_after_cancellation_start(
                test_state: &mut TestState,
            ) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
            async fn test_public_withdraw_tokens_by_taker(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (escrow, escrow_ata) = create_escrow(test_state).await;

                let transaction = SrcProgram::get_public_withdraw_tx(
                    test_state,
                    &escrow,
                    &escrow_ata,
                    &test_state.taker_wallet.keypair,
                );

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
                );

                let escrow_data_len = DEFAULT_SRC_ESCROW_SIZE;

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
                                test_state.taker_wallet.keypair.pubkey(),
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
            async fn test_public_withdraw_tokens_any_resolver(test_state: &mut TestState) {
                create_order(test_state).await;
                let withdrawer = Keypair::new();
                prepare_resolvers(
                    test_state,
                    &[
                        test_state.taker_wallet.keypair.pubkey(),
                        withdrawer.pubkey(),
                    ],
                )
                .await;
                transfer_lamports(
                    &mut test_state.context,
                    WALLET_DEFAULT_LAMPORTS,
                    &test_state.payer_kp,
                    &withdrawer.pubkey(),
                )
                .await;
                let (escrow, escrow_ata) = create_escrow(test_state).await;

                let transaction = SrcProgram::get_public_withdraw_tx(
                    test_state,
                    &escrow,
                    &escrow_ata,
                    &withdrawer,
                );

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
                );

                let escrow_data_len = DEFAULT_SRC_ESCROW_SIZE;

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
                                test_state.taker_wallet.keypair.pubkey(),
                                rent_lamports + token_account_rent
                                    - test_state.test_arguments.safety_deposit,
                            ),
                            native_change(
                                withdrawer.pubkey(),
                                test_state.test_arguments.safety_deposit,
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
            async fn test_public_withdraw_fails_with_wrong_secret(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(
                    test_state,
                    &[
                        test_state.taker_wallet.keypair.pubkey(),
                        test_state.context.payer.pubkey(),
                    ],
                )
                .await;
                common_escrow_tests::test_public_withdraw_fails_with_wrong_secret(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_with_wrong_taker_ata(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_public_withdraw_fails_with_wrong_taker_ata(test_state)
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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
                prepare_resolvers(
                    test_state,
                    &[
                        test_state.taker_wallet.keypair.pubkey(),
                        test_state.context.payer.pubkey(),
                    ],
                )
                .await;
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
                prepare_resolvers(
                    test_state,
                    &[
                        test_state.taker_wallet.keypair.pubkey(),
                        test_state.context.payer.pubkey(),
                    ],
                )
                .await;
                common_escrow_tests::test_public_withdraw_fails_after_cancellation_start(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_without_resolver_access(
                test_state: &mut TestState,
            ) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (escrow, escrow_ata) = create_escrow(test_state).await;

                let withdrawer = Keypair::new();
                // withdrawer does not have resolver access
                let transaction = SrcProgram::get_public_withdraw_tx(
                    test_state,
                    &escrow,
                    &escrow_ata,
                    &withdrawer,
                );

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error((
                        0,
                        ProgramError::Custom(ErrorCode::AccountNotInitialized.into()),
                    ));
            }
        }

        mod test_escrow_cancel {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_cancel(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_with_excess_tokens(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                let (escrow, escrow_ata) = create_escrow(test_state).await;
                let transaction = SrcProgram::get_cancel_tx(test_state, &escrow, &escrow_ata);

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
                );

                let (maker_ata, _) = find_user_ata(test_state);

                let excess_amount = 1000;
                // Send excess tokens to the escrow account
                helpers_src::mint_excess_tokens(test_state, &escrow_ata, excess_amount).await;

                test_state
                    .expect_balance_change(
                        transaction,
                        &[token_change(
                            maker_ata,
                            test_state.test_arguments.escrow_amount + excess_amount,
                        )],
                    )
                    .await;

                let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
                assert!(acc_lookup_result.is_none());

                let acc_lookup_result = test_state.client.get_account(escrow).await.unwrap();
                assert!(acc_lookup_result.is_none());
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_cancel_by_non_creator(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_cannot_cancel_by_non_creator(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_cancel_with_wrong_maker_ata(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_cannot_cancel_with_wrong_maker_ata(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_cancel_with_wrong_escrow_ata(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_cannot_cancel_before_cancellation_start(test_state).await
            }
        }

        mod test_order_cancel {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_cancel(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                helpers_src::test_order_cancel(test_state).await;
            }
        }

        mod test_order_cancel_with_excess_tokens {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_cancel(test_state: &mut TestState) {
                let (order, order_ata) = create_order(test_state).await;
                let transaction = get_cancel_order_tx(test_state, &order, &order_ata, None);

                let (maker_ata, _) = find_user_ata(test_state);

                let excess_amount = 1000;
                // Send excess tokens to the order account
                helpers_src::mint_excess_tokens(test_state, &order_ata, excess_amount).await;

                let balance_changes: Vec<BalanceChange> = vec![token_change(
                    maker_ata,
                    test_state.test_arguments.order_amount + excess_amount,
                )];

                test_state
                    .expect_balance_change(transaction, &balance_changes)
                    .await;

                let acc_lookup_result = test_state.client.get_account(order).await.unwrap();
                assert!(acc_lookup_result.is_none());

                let acc_lookup_result = test_state.client.get_account(order_ata).await.unwrap();
                assert!(acc_lookup_result.is_none());
            }
        }

        mod test_order_cancel_by_resolver {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_by_resolver_for_free_at_the_auction_start(
                test_state: &mut TestState,
            ) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                helpers_src::test_cancel_by_resolver_for_free_at_the_auction_start(test_state)
                    .await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_by_resolver_for_free_at_the_auction_start_with_excess_tokens(
                test_state: &mut TestState,
            ) {
                let (order, order_ata) = create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let transaction =
                    get_cancel_order_by_resolver_tx(test_state, &order, &order_ata, None);

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp + test_state.test_arguments.expiration_duration,
                );

                let (maker_ata, _) = find_user_ata(test_state);

                let excess_amount = 1000;
                // Send excess tokens to the order account
                helpers_src::mint_excess_tokens(test_state, &order_ata, excess_amount).await;

                let balance_changes: Vec<BalanceChange> = vec![token_change(
                    maker_ata,
                    test_state.test_arguments.order_amount + excess_amount,
                )];

                test_state
                    .expect_balance_change(transaction, &balance_changes)
                    .await;

                let acc_lookup_result = test_state.client.get_account(order).await.unwrap();
                assert!(acc_lookup_result.is_none());

                let acc_lookup_result = test_state.client.get_account(order_ata).await.unwrap();
                assert!(acc_lookup_result.is_none());
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_by_resolver_at_different_points(test_state: &mut TestState) {
                helpers_src::test_cancel_by_resolver_at_different_points(test_state, false, None)
                    .await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_by_resolver_after_auction(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                helpers_src::test_cancel_by_resolver_after_auction(test_state).await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_by_resolver_reward_less_then_auction_calculated(
                test_state: &mut TestState,
            ) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                helpers_src::test_cancel_by_resolver_reward_less_then_auction_calculated(
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

                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_cancel_by_taker(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                test_public_cancel_escrow(
                    test_state,
                    &test_state.taker_wallet.keypair.insecure_clone(),
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

                prepare_resolvers(
                    test_state,
                    &[test_state.taker_wallet.keypair.pubkey(), canceller.pubkey()],
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
                prepare_resolvers(
                    test_state,
                    &[
                        test_state.taker_wallet.keypair.pubkey(),
                        test_state.payer_kp.pubkey(),
                    ],
                )
                .await;

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
            async fn test_rescue_all_tokens_from_order_and_close_ata(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                helpers_src::test_rescue_all_tokens_from_order_and_close_ata(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_rescue_part_of_tokens_from_order_and_not_close_ata(
                test_state: &mut TestState,
            ) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                helpers_src::test_rescue_part_of_tokens_from_order_and_not_close_ata(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_from_order_before_rescue_delay_pass(
                test_state: &mut TestState,
            ) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                helpers_src::test_cannot_rescue_funds_from_order_before_rescue_delay_pass(
                    test_state,
                )
                .await
            }

            // #[test_context(TestState)]
            // #[tokio::test]
            // async fn test_cannot_rescue_funds_from_order_by_non_recipient(test_state: &mut TestState) { // TODO: return after implement whitelist
            //     helpers_src::test_cannot_rescue_funds_from_order_by_non_recipient(test_state).await
            // }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_from_order_with_wrong_taker_ata(
                test_state: &mut TestState,
            ) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                helpers_src::test_cannot_rescue_funds_from_order_with_wrong_taker_ata(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_from_order_with_wrong_order_ata(
                test_state: &mut TestState,
            ) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                helpers_src::test_cannot_rescue_funds_from_order_with_wrong_orders_ata(test_state)
                    .await
            }
        }

        mod test_order_rescue_funds_for_escrow {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_rescue_all_tokens_and_close_ata(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(
                    test_state,
                    &[
                        test_state.taker_wallet.keypair.pubkey(),
                        test_state.maker_wallet.keypair.pubkey(),
                    ],
                )
                .await;
                common_escrow_tests::test_rescue_all_tokens_and_close_ata(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_rescue_part_of_tokens_and_not_close_ata(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(
                    test_state,
                    &[
                        test_state.taker_wallet.keypair.pubkey(),
                        test_state.maker_wallet.keypair.pubkey(),
                    ],
                )
                .await;
                common_escrow_tests::test_rescue_part_of_tokens_and_not_close_ata(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_before_rescue_delay_pass(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(
                    test_state,
                    &[
                        test_state.taker_wallet.keypair.pubkey(),
                        test_state.maker_wallet.keypair.pubkey(),
                    ],
                )
                .await;
                common_escrow_tests::test_cannot_rescue_funds_before_rescue_delay_pass(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_by_non_recipient(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(
                    test_state,
                    &[
                        test_state.taker_wallet.keypair.pubkey(),
                        test_state.maker_wallet.keypair.pubkey(),
                    ],
                )
                .await;
                common_escrow_tests::test_cannot_rescue_funds_by_non_recipient(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_with_wrong_taker_ata(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(
                    test_state,
                    &[
                        test_state.taker_wallet.keypair.pubkey(),
                        test_state.maker_wallet.keypair.pubkey(),
                    ],
                )
                .await;
                common_escrow_tests::test_cannot_rescue_funds_with_wrong_taker_ata(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_with_wrong_order_ata(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(
                    test_state,
                    &[
                        test_state.taker_wallet.keypair.pubkey(),
                        test_state.maker_wallet.keypair.pubkey(),
                    ],
                )
                .await;
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
        mod test_partial_fill_escrow_creation {

            use super::*;
            use cross_chain_escrow_src::merkle_tree::MerkleProof;
            use solana_sdk::keccak::{hashv, Hash};

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_create_escrow_with_merkle_proof_and_leaf_validation(
                test_state: &mut TestState,
            ) {
                let (order, order_ata) = create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;
                test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_escrow_creation_for_partial_fill(test_state, DEFAULT_ESCROW_AMOUNT).await;

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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;
                test_escrow_creation_for_partial_fill(
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
            async fn test_create_two_escrows_for_full_order_excess_tokens(
                test_state: &mut TestState,
            ) {
                let (order, order_ata) = create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                let excess_amount = 1000;
                // Send excess tokens to the order ATA.
                mint_excess_tokens(test_state, &order_ata, excess_amount).await;
                let (_, escrow_ata) = test_escrow_creation_for_partial_fill(
                    test_state,
                    DEFAULT_ESCROW_AMOUNT - escrow_amount, // full fill
                )
                .await;

                // Check that the escrow ATA was created with the correct amount.
                assert_eq!(
                    test_state.test_arguments.escrow_amount + excess_amount,
                    get_token_balance(&mut test_state.context, &escrow_ata).await
                );

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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;
                let (_, _, transaction) = test_escrow_creation_for_partial_fill_data(
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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;
                let so_small_escrow_amount = 1;
                let (_, _, transaction) =
                    test_escrow_creation_for_partial_fill_data(test_state, so_small_escrow_amount)
                        .await;

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
            async fn test_create_escrow_fails_with_incorrect_merkle_root(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.order_parts_amount = DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                let merkle_hashes = compute_merkle_leaves(test_state);
                test_state.hashlock = hashv(&[b"incorrect_root"]);
                test_state.test_arguments.allow_multiple_fills = true;
                create_order(test_state).await;

                test_state.test_arguments.escrow_amount =
                    DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                let index_to_validate = get_index_for_escrow_amount(
                    test_state,
                    test_state.test_arguments.escrow_amount,
                );
                let hashed_secret = merkle_hashes.hashed_secrets[index_to_validate];
                let proof_hashes = get_proof(merkle_hashes.leaves.clone(), index_to_validate);
                let proof = MerkleProof {
                    proof: proof_hashes,
                    index: index_to_validate as u64,
                    hashed_secret,
                };
                test_state.test_arguments.merkle_proof = Some(proof);
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
            async fn test_create_escrow_fails_with_incorrect_secret_for_leaf(
                test_state: &mut TestState,
            ) {
                create_order_for_partial_fill(test_state).await;

                let merkle_hashes = compute_merkle_leaves(test_state);
                test_state.test_arguments.escrow_amount =
                    DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 2;
                let index_to_validate = get_index_for_escrow_amount(
                    test_state,
                    test_state.test_arguments.escrow_amount,
                );
                let proof_hashes = get_proof(merkle_hashes.leaves.clone(), index_to_validate);

                // Incorrect hashed_secret
                let proof = MerkleProof {
                    proof: proof_hashes,
                    index: index_to_validate as u64,
                    hashed_secret: merkle_hashes.hashed_secrets[index_to_validate + 1],
                };
                test_state.test_arguments.merkle_proof = Some(proof);
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

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

                let index_to_validate = get_index_for_escrow_amount(
                    test_state,
                    test_state.test_arguments.escrow_amount,
                ); // fill the full order
                let hashed_secret = merkle_hashes.hashed_secrets[index_to_validate];
                let proof_hashes = get_proof(merkle_hashes.leaves.clone(), index_to_validate);
                let proof = MerkleProof {
                    proof: proof_hashes,
                    index: index_to_validate as u64,
                    hashed_secret,
                };
                test_state.test_arguments.merkle_proof = Some(proof);
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
    }
);

// Native Mint (wrapped SOL) is always owned by the SPL Token program
type TestState = TestStateBase<SrcProgram, TokenSPL>;

mod test_native_src {
    use anchor_lang::Space;
    use solana_program_pack::Pack;

    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_order_creation(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        helpers_src::test_order_creation(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        // Check the lamport X of escrow account is as expected.
        let escrow_data_len = cross_chain_escrow_src::constants::DISCRIMINATOR_BYTES
            + cross_chain_escrow_src::EscrowSrc::INIT_SPACE;
        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;
        assert_eq!(
            rent_lamports,
            test_state.client.get_balance(escrow).await.unwrap()
        );

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;

        // Check token balance for the escrow account is as expected.
        assert_eq!(
            DEFAULT_ESCROW_AMOUNT + token_account_rent,
            test_state.client.get_balance(escrow_ata).await.unwrap()
        );

        // Check native balance for the creator is as expected.
        assert_eq!(
            WALLET_DEFAULT_LAMPORTS - token_account_rent - rent_lamports,
            // The pure lamport balance of the creator wallet after the transaction.
            test_state
                .client
                .get_balance(test_state.taker_wallet.keypair.pubkey())
                .await
                .unwrap()
        );
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
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;
        let transaction = SrcProgram::get_withdraw_tx(test_state, &escrow, &escrow_ata);

        let token_account_rent = get_min_rent_for_size(
            &mut test_state.client,
            get_token_account_len(PhantomData::<TestState>),
        )
        .await;

        let escrow_rent =
            get_min_rent_for_size(&mut test_state.client, DEFAULT_SRC_ESCROW_SIZE).await;

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
        );

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(
                        test_state.taker_wallet.keypair.pubkey(),
                        token_account_rent + escrow_rent,
                    ),
                    token_change(
                        test_state.taker_wallet.native_token_account,
                        test_state.test_arguments.escrow_amount,
                    ),
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
    async fn test_public_withdraw_by_taker(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;
        let withdrawer = test_state.taker_wallet.keypair.insecure_clone();
        prepare_resolvers(test_state, &[withdrawer.pubkey()]).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction =
            SrcProgram::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp
                + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
        );

        let escrow_data_len = cross_chain_escrow_src::constants::DISCRIMINATOR_BYTES
            + cross_chain_escrow_src::EscrowSrc::INIT_SPACE;

        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(
                        test_state.taker_wallet.keypair.pubkey(),
                        rent_lamports + token_account_rent,
                    ),
                    token_change(
                        test_state.taker_wallet.native_token_account,
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
    async fn test_public_withdraw_by_any_resolver(test_state: &mut TestState) {
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
        prepare_resolvers(
            test_state,
            &[
                withdrawer.pubkey(),
                test_state.taker_wallet.keypair.pubkey(),
            ],
        )
        .await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction =
            SrcProgram::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp
                + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
        );

        let escrow_data_len = cross_chain_escrow_src::constants::DISCRIMINATOR_BYTES
            + cross_chain_escrow_src::EscrowSrc::INIT_SPACE;

        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(
                        test_state.taker_wallet.keypair.pubkey(),
                        rent_lamports + token_account_rent
                            - test_state.test_arguments.safety_deposit,
                    ),
                    native_change(
                        withdrawer.pubkey(),
                        test_state.test_arguments.safety_deposit,
                    ),
                    token_change(
                        test_state.taker_wallet.native_token_account,
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
    async fn test_order_cancel(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_order_cancel(test_state).await;
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
            Some(&test_state.maker_wallet.native_token_account),
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
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_cancel_by_resolver_for_free_at_the_auction_start(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_at_different_points(test_state: &mut TestState) {
        helpers_src::test_cancel_by_resolver_at_different_points(
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
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_cancel_by_resolver_after_auction(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_reward_less_then_auction_calculated(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_cancel_by_resolver_reward_less_then_auction_calculated(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_fails_if_native_and_maker_ata_provided(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        let (order, order_ata) = create_order(test_state).await;

        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        set_time(
            &mut test_state.context,
            test_state.init_timestamp + test_state.test_arguments.expiration_duration,
        );

        let transaction = get_cancel_order_by_resolver_tx(
            test_state,
            &order,
            &order_ata,
            Some(&test_state.maker_wallet.native_token_account),
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
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;
        let transaction = SrcProgram::get_cancel_tx(test_state, &escrow, &escrow_ata);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
        );

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;
        let escrow_rent =
            get_min_rent_for_size(&mut test_state.client, DEFAULT_SRC_ESCROW_SIZE).await;

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(
                        test_state.maker_wallet.keypair.pubkey(),
                        test_state.test_arguments.escrow_amount,
                    ),
                    native_change(
                        test_state.taker_wallet.keypair.pubkey(),
                        escrow_rent + token_account_rent,
                    ),
                ],
            )
            .await;

        let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
        assert!(acc_lookup_result.is_none());

        let acc_lookup_result = test_state.client.get_account(escrow).await.unwrap();
        assert!(acc_lookup_result.is_none());
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_cancel_by_any_account(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;

        let canceller = Keypair::new();
        prepare_resolvers(
            test_state,
            &[test_state.taker_wallet.keypair.pubkey(), canceller.pubkey()],
        )
        .await;

        let (escrow, escrow_ata) = create_escrow(test_state).await;
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

        let escrow_data_len = DEFAULT_SRC_ESCROW_SIZE;
        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, SplTokenAccount::LEN).await;

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(canceller.pubkey(), test_state.test_arguments.safety_deposit),
                    native_change(
                        test_state.maker_wallet.keypair.pubkey(),
                        test_state.test_arguments.escrow_amount,
                    ),
                    native_change(
                        test_state.taker_wallet.keypair.pubkey(),
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
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        common_escrow_tests::test_rescue_all_tokens_and_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_part_of_tokens_and_not_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        common_escrow_tests::test_rescue_part_of_tokens_and_not_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_all_tokens_from_order_and_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_rescue_all_tokens_from_order_and_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_part_of_tokens_from_order_and_not_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_rescue_part_of_tokens_from_order_and_not_close_ata(test_state).await
    }
}

mod test_wrapped_native {
    use anchor_lang::Space;
    use solana_program_pack::Pack;

    use super::*;
    use crate::helpers_src::create_public_escrow_cancel_tx;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_order_creation(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_order_creation(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order(test_state).await;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        common_escrow_tests::test_escrow_creation(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        create_order(test_state).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;
        let transaction = SrcProgram::get_withdraw_tx(test_state, &escrow, &escrow_ata);

        let token_account_rent = get_min_rent_for_size(
            &mut test_state.client,
            get_token_account_len(PhantomData::<TestState>),
        )
        .await;

        let escrow_rent =
            get_min_rent_for_size(&mut test_state.client, DEFAULT_SRC_ESCROW_SIZE).await;

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
        );

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(
                        test_state.taker_wallet.keypair.pubkey(),
                        token_account_rent + escrow_rent,
                    ),
                    token_change(
                        test_state.taker_wallet.native_token_account,
                        test_state.test_arguments.escrow_amount,
                    ),
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
    async fn test_public_withdraw_by_taker(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order(test_state).await;
        let withdrawer = test_state.taker_wallet.keypair.insecure_clone();
        prepare_resolvers(test_state, &[withdrawer.pubkey()]).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction =
            SrcProgram::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp
                + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
        );

        let escrow_data_len = cross_chain_escrow_src::constants::DISCRIMINATOR_BYTES
            + cross_chain_escrow_src::EscrowSrc::INIT_SPACE;

        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(
                        test_state.taker_wallet.keypair.pubkey(),
                        rent_lamports + token_account_rent,
                    ),
                    token_change(
                        test_state.taker_wallet.native_token_account,
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
    async fn test_public_withdraw_by_any_resolver(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order(test_state).await;
        let withdrawer = Keypair::new();
        transfer_lamports(
            &mut test_state.context,
            WALLET_DEFAULT_LAMPORTS,
            &test_state.payer_kp,
            &withdrawer.pubkey(),
        )
        .await;
        prepare_resolvers(
            test_state,
            &[
                withdrawer.pubkey(),
                test_state.taker_wallet.keypair.pubkey(),
            ],
        )
        .await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction =
            SrcProgram::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp
                + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
        );

        let escrow_data_len = cross_chain_escrow_src::constants::DISCRIMINATOR_BYTES
            + cross_chain_escrow_src::EscrowSrc::INIT_SPACE;

        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(
                        test_state.taker_wallet.keypair.pubkey(),
                        rent_lamports + token_account_rent
                            - test_state.test_arguments.safety_deposit,
                    ),
                    native_change(
                        withdrawer.pubkey(),
                        test_state.test_arguments.safety_deposit,
                    ),
                    token_change(
                        test_state.taker_wallet.native_token_account,
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
    async fn test_order_cancel(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        helpers_src::test_order_cancel(test_state).await;
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
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_cancel_by_resolver_for_free_at_the_auction_start(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_at_different_points(test_state: &mut TestState) {
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_cancel_by_resolver_at_different_points(
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
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_cancel_by_resolver_after_auction(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_reward_less_then_auction_calculated(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_cancel_by_resolver_reward_less_then_auction_calculated(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_fails_if_wrapped_and_maker_ata_not_provided(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        let (order, order_ata) = create_order(test_state).await;

        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        common_escrow_tests::test_cancel(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_cancel_by_any_account(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;

        create_order(test_state).await;

        let canceller = Keypair::new();
        prepare_resolvers(
            test_state,
            &[test_state.taker_wallet.keypair.pubkey(), canceller.pubkey()],
        )
        .await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

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

        let escrow_data_len = DEFAULT_SRC_ESCROW_SIZE;
        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, SplTokenAccount::LEN).await;

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(canceller.pubkey(), test_state.test_arguments.safety_deposit),
                    token_change(
                        test_state.maker_wallet.native_token_account,
                        test_state.test_arguments.escrow_amount,
                    ),
                    native_change(
                        test_state.taker_wallet.keypair.pubkey(),
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
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        common_escrow_tests::test_rescue_all_tokens_and_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_part_of_tokens_and_not_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order(test_state).await;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        common_escrow_tests::test_rescue_part_of_tokens_and_not_close_ata(test_state).await
    }
}
