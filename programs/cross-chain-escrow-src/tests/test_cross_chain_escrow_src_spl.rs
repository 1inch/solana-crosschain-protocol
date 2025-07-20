use anchor_lang::{error::ErrorCode, prelude::ProgramError};
use common::{error::EscrowError, timelocks::Stage};
use common_tests::helpers::*;
use common_tests::run_for_tokens;
use common_tests::src_program::{
    create_order, create_order_data, create_public_escrow_cancel_tx,
    get_cancel_order_by_resolver_tx, get_cancel_order_tx, get_create_order_tx, get_order_addresses,
    get_rescue_funds_from_order_tx, SrcProgram,
};
use common_tests::tests as common_escrow_tests;
use common_tests::whitelist::prepare_resolvers;
use solana_program_test::tokio;
use solana_sdk::signature::Signer;
use solana_sdk::signer::keypair::Keypair;
use test_context::test_context;

use primitive_types::U256;

pub mod helpers_src;
use helpers_src::*;

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
            async fn test_order_creation_with_pre_existing_order_ata(test_state: &mut TestState) {
                let (order_pda, _) = get_order_addresses(test_state);

                let _order_ata =
                    <TestState as HasTokenVariant>::Token::initialize_spl_associated_account(
                        &mut test_state.context,
                        &test_state.token,
                        &order_pda,
                    )
                    .await;
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
                    .expect_error(ProgramError::Custom(
                        EscrowError::ZeroAmountOrDeposit.into(),
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
                    .expect_error(ProgramError::Custom(
                        EscrowError::ZeroAmountOrDeposit.into(),
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
                    .expect_error(ProgramError::Custom(
                        EscrowError::SafetyDepositTooLarge.into(),
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
                    .expect_error(ProgramError::Custom(
                        solana_sdk::system_instruction::SystemError::AccountAlreadyInUse as u32,
                    ));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fails_if_after_expiration(test_state: &mut TestState) {
                test_state.test_arguments.expiration_time = test_state.init_timestamp - 1;
                let (order, order_ata, tx) = create_order_data(test_state);

                test_state
                    .client
                    .process_transaction(tx)
                    .await
                    .expect_error(ProgramError::Custom(EscrowError::OrderHasExpired.into()));

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
                let token_account_rent = get_min_rent_for_size(
                    &mut test_state.client,
                    <TestState as HasTokenVariant>::Token::get_token_account_size(),
                )
                .await;

                test_state.test_arguments.max_cancellation_premium = token_account_rent + 1;
                test_state.order_hash = common_tests::src_program::get_order_hash(test_state);
                let (order, order_ata) = get_order_addresses(test_state);

                let transaction = get_create_order_tx(test_state, &order, &order_ata);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(
                        EscrowError::InvalidCancellationFee.into(),
                    ));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fails_with_incorrect_parts_for_allow_multiples_fills(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.allow_multiple_fills = true;
                let too_small_parts_amount = 1;
                test_state.hashlock = prepare_hashlock_for_root(
                    test_state.hashlock.to_bytes(),
                    too_small_parts_amount,
                );
                let (_, _, transaction) = create_order_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(EscrowError::InvalidPartsAmount.into()));
            }
        }

        mod test_escrow_creation {
            use super::*;

            const AUCTION_START_OFFSET: u32 = 250;
            const AUCTION_DURATION: u32 = 1000;
            const INITIAL_RATE_BUMP: u32 = 10_000; // 10%
            const INTERMEDIATE_RATE_BUMP: u32 = 9_000; // 9%
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
            async fn test_escrow_creation_with_pre_existing_escrow_ata(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (escrow_pda, _) = get_escrow_addresses(test_state);

                let _escrow_ata =
                    <TestState as HasTokenVariant>::Token::initialize_spl_associated_account(
                        &mut test_state.context,
                        &test_state.token,
                        &escrow_pda,
                    )
                    .await;
                common_escrow_tests::test_escrow_creation(test_state).await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_with_dutch_auction_params(test_state: &mut TestState) {
                test_state.test_arguments.dutch_auction_data =
                    cross_chain_escrow_src::AuctionData {
                        start_time: test_state.init_timestamp - AUCTION_START_OFFSET,
                        duration: AUCTION_DURATION,
                        initial_rate_bump: INITIAL_RATE_BUMP.into(),
                        points_and_time_deltas: vec![
                            cross_chain_escrow_src::auction::PointAndTimeDelta {
                                rate_bump: INTERMEDIATE_RATE_BUMP.into(),
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
                type S = <TestState as HasTokenVariant>::Token;

                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (_, order_ata) = create_order(test_state).await;
                let excess_amount = 1000;
                // Send excess tokens to the order ATA.
                S::mint_spl_tokens(
                    &mut test_state.context,
                    &test_state.token,
                    &order_ata,
                    &test_state.payer_kp.pubkey(),
                    &test_state.payer_kp,
                    excess_amount,
                )
                .await;
                let (_, escrow_ata) = create_escrow(test_state).await;

                // Check that the escrow ATA was created with the correct amount.
                assert_eq!(
                    test_state.test_arguments.escrow_amount + excess_amount,
                    get_token_balance(&mut test_state.context, &escrow_ata).await
                );

                // Check that the order ATA was closed.
                let order_ata_account = test_state.client.get_account(order_ata).await.unwrap();
                assert!(order_ata_account.is_none());
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
                        initial_rate_bump: INITIAL_RATE_BUMP.into(),
                        points_and_time_deltas: vec![
                            cross_chain_escrow_src::auction::PointAndTimeDelta {
                                rate_bump: INTERMEDIATE_RATE_BUMP.into(),
                                time_delta: INTERMEDIATE_TIME_DELTA,
                            },
                        ],
                    };

                create_order(test_state).await;
                test_state.test_arguments.dutch_auction_data =
                    cross_chain_escrow_src::AuctionData {
                        start_time: test_state.init_timestamp - AUCTION_START_OFFSET,
                        duration: AUCTION_DURATION,
                        initial_rate_bump: INITIAL_RATE_BUMP.into(),
                        points_and_time_deltas: vec![
                            cross_chain_escrow_src::auction::PointAndTimeDelta {
                                rate_bump: (INTERMEDIATE_RATE_BUMP * 2).into(), // Incorrect rate bump
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
                    .expect_error(ProgramError::Custom(
                        EscrowError::DutchAuctionDataHashMismatch.into(),
                    ));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_calculation_of_dutch_auction_params(test_state: &mut TestState) {
                test_state.test_arguments.dutch_auction_data =
                    cross_chain_escrow_src::AuctionData {
                        start_time: test_state.init_timestamp - AUCTION_START_OFFSET,
                        duration: AUCTION_DURATION,
                        initial_rate_bump: INITIAL_RATE_BUMP.into(),
                        points_and_time_deltas: vec![
                            cross_chain_escrow_src::auction::PointAndTimeDelta {
                                rate_bump: INTERMEDIATE_RATE_BUMP.into(), // 9%
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
                    .expect_error(ProgramError::Custom(
                        ErrorCode::AccountNotInitialized.into(),
                    ));

                // Check that the order accounts have not been created.
                let acc_lookup_result = test_state.client.get_account(escrow).await.unwrap();
                assert!(acc_lookup_result.is_none());

                let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
                assert!(acc_lookup_result.is_none());
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_with_expired_order(test_state: &mut TestState) {
                create_order(test_state).await;

                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                set_time(
                    &mut test_state.context,
                    test_state.test_arguments.expiration_time + 1,
                );

                let (_, _, transaction) = create_escrow_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(EscrowError::OrderHasExpired.into()));
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
                    .expect_error(ProgramError::Custom(EscrowError::InvalidAmount.into()));
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
                    .expect_error(ProgramError::Custom(
                        ErrorCode::AccountNotInitialized.into(),
                    ));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fails_with_incorrect_token(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                test_state.token = <TestState as HasTokenVariant>::Token::deploy_spl_token(
                    &mut test_state.context,
                )
                .await
                .pubkey();
                let (_, _, transaction) = create_escrow_data(test_state);
                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(
                        ErrorCode::AccountNotInitialized.into(),
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
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                helpers_src::test_withdraw_escrow(test_state, &escrow, &escrow_ata).await;
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
                    test_state
                        .test_arguments
                        .src_timelocks
                        .get(Stage::SrcWithdrawal)
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

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_fails_with_incorrect_token(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (escrow, escrow_ata) = create_escrow(test_state).await;

                test_state.token = <TestState as HasTokenVariant>::Token::deploy_spl_token(
                    &mut test_state.context,
                )
                .await
                .pubkey();

                let transaction = SrcProgram::get_withdraw_tx(test_state, &escrow, &escrow_ata);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(ErrorCode::ConstraintAssociated.into()));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_fails_if_public_withdrawal_duration_overflows(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.src_timelocks =
                    init_timelocks(0, u32::MAX, 0, 0, 0, 0, 0, 0);
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                let transaction = SrcProgram::get_public_withdraw_tx(
                    test_state,
                    &escrow,
                    &escrow_ata,
                    &test_state.taker_wallet.keypair,
                );

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::ArithmeticOverflow);
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
                    test_state
                        .test_arguments
                        .src_timelocks
                        .get(Stage::SrcPublicWithdrawal)
                        .unwrap(),
                );

                let escrow_data_len = DEFAULT_SRC_ESCROW_SIZE;

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
                    test_state
                        .test_arguments
                        .src_timelocks
                        .get(Stage::SrcPublicWithdrawal)
                        .unwrap(),
                );

                let escrow_data_len = DEFAULT_SRC_ESCROW_SIZE;

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
                    .expect_error(ProgramError::Custom(
                        ErrorCode::AccountNotInitialized.into(),
                    ));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_with_incorrect_token(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (escrow, escrow_ata) = create_escrow(test_state).await;

                test_state.token = <TestState as HasTokenVariant>::Token::deploy_spl_token(
                    &mut test_state.context,
                )
                .await
                .pubkey();
                let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

                let transaction =
                    SrcProgram::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &taker_kp);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(ErrorCode::ConstraintAssociated.into()));
            }
        }

        mod test_escrow_cancel {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                common_escrow_tests::test_cancel(test_state, &escrow, &escrow_ata).await
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
                    test_state
                        .test_arguments
                        .src_timelocks
                        .get(Stage::SrcCancellation)
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
                            account_closure(escrow, true),
                            account_closure(escrow_ata, true),
                        ],
                    )
                    .await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_cancel_by_non_maker(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_cannot_cancel_by_non_maker(test_state).await
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
                    .expect_error(ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()))
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_cancel_before_cancellation_start(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_cannot_cancel_before_cancellation_start(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_fails_with_incorrect_token(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (escrow, escrow_ata) = create_escrow(test_state).await;

                test_state.token = <TestState as HasTokenVariant>::Token::deploy_spl_token(
                    &mut test_state.context,
                )
                .await
                .pubkey();

                let transaction = SrcProgram::get_cancel_tx(test_state, &escrow, &escrow_ata);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(ErrorCode::ConstraintAssociated.into()));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_fails_if_cancellation_duration_overflows(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.src_timelocks =
                    init_timelocks(0, 0, u32::MAX, 0, 0, 0, 0, 0);
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                let transaction = SrcProgram::get_cancel_tx(test_state, &escrow, &escrow_ata);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::ArithmeticOverflow);
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

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_rescue_tokens_when_order_is_deleted(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                helpers_src::test_rescue_tokens_when_order_is_deleted(test_state).await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_cancel_fails_with_incorrect_token(test_state: &mut TestState) {
                let (order, order_ata) = create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_state.token = <TestState as HasTokenVariant>::Token::deploy_spl_token(
                    &mut test_state.context,
                )
                .await
                .pubkey();

                let transaction = get_cancel_order_tx(test_state, &order, &order_ata, None);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(EscrowError::InvalidMint.into()));
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
                mint_excess_tokens(test_state, &order_ata, excess_amount).await;

                let balance_changes: Vec<StateChange> = vec![
                    token_change(
                        maker_ata,
                        test_state.test_arguments.order_amount + excess_amount,
                    ),
                    account_closure(order, true),
                    account_closure(order_ata, true),
                ];

                test_state
                    .expect_state_change(transaction, &balance_changes)
                    .await;
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

            // Checks the maker_amount transfer transaction is skipped if the maker_amount is zero
            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_by_resolver_with_zero_maker_amount(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                let token_account_rent = get_min_rent_for_size(
                    &mut test_state.client,
                    <TestState as HasTokenVariant>::Token::get_token_account_size(),
                )
                .await;

                // By setting these two test arguments below to token_account_rent and current time to
                // more than (test_state.test_arguments.expiration_time +
                // auction_duration)
                // we ensure that the make_amount will evaluate to zero.
                test_state.test_arguments.max_cancellation_premium = token_account_rent;
                test_state.test_arguments.reward_limit = token_account_rent;

                let (order, order_ata) = create_order(test_state).await;
                let transaction =
                    get_cancel_order_by_resolver_tx(test_state, &order, &order_ata, None);

                set_time(
                    &mut test_state.context,
                    test_state.test_arguments.expiration_time
                        + test_state.test_arguments.cancellation_auction_duration
                        + 1,
                );

                let result = test_state
                    .client
                    .simulate_transaction(transaction)
                    .await
                    .expect("Simulation RPC failed");

                // Extract the simulation details
                let sim_details = result
                    .simulation_details
                    .expect("Simulation details not found");

                // Check it does not contain system program invocation for the transfer transaction
                assert!(!sim_details
                    .logs
                    .contains(&"Program 11111111111111111111111111111111 invoke [1]".to_string()));
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
                    test_state.test_arguments.expiration_time,
                );

                let (maker_ata, _) = find_user_ata(test_state);

                let excess_amount = 1000;
                // Send excess tokens to the order account
                mint_excess_tokens(test_state, &order_ata, excess_amount).await;

                let balance_changes: Vec<StateChange> = vec![
                    token_change(
                        maker_ata,
                        test_state.test_arguments.order_amount + excess_amount,
                    ),
                    account_closure(order, true),
                    account_closure(order_ata, true),
                ];

                test_state
                    .expect_state_change(transaction, &balance_changes)
                    .await;
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
                    .expect_error(ProgramError::Custom(EscrowError::OrderNotExpired.into()));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_by_resolver_fails_with_incorrect_token(
                test_state: &mut TestState,
            ) {
                let (order, order_ata) = create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_state.token = <TestState as HasTokenVariant>::Token::deploy_spl_token(
                    &mut test_state.context,
                )
                .await
                .pubkey();

                let transaction =
                    get_cancel_order_by_resolver_tx(test_state, &order, &order_ata, None);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(EscrowError::InvalidMint.into()));
            }
        }

        mod test_order_public_cancel {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_cancel_by_taker(test_state: &mut TestState) {
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                create_order(test_state).await;
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                test_public_cancel_escrow(
                    test_state,
                    &escrow,
                    &escrow_ata,
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

                create_order(test_state).await;
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                test_public_cancel_escrow(test_state, &escrow, &escrow_ata, &canceller).await;
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
                    test_state
                        .test_arguments
                        .src_timelocks
                        .get(Stage::SrcCancellation)
                        .unwrap(),
                );
                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(EscrowError::InvalidTime.into()))
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_cancel_fails_with_incorrect_token(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (escrow, escrow_ata) = create_escrow(test_state).await;

                test_state.token = <TestState as HasTokenVariant>::Token::deploy_spl_token(
                    &mut test_state.context,
                )
                .await
                .pubkey();
                let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

                let transaction =
                    create_public_escrow_cancel_tx(test_state, &escrow, &escrow_ata, &taker_kp);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(ErrorCode::ConstraintAssociated.into()));
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
                type S = <TestState as HasTokenVariant>::Token;

                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (order, _) = create_order(test_state).await;

                let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
                let order_ata = S::initialize_spl_associated_account(
                    &mut test_state.context,
                    &token_to_rescue,
                    &order,
                )
                .await;
                let taker_ata = S::initialize_spl_associated_account(
                    &mut test_state.context,
                    &token_to_rescue,
                    &test_state.taker_wallet.keypair.pubkey(),
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
                    &taker_ata,
                );

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp + common::constants::RESCUE_DELAY - 100,
                );

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(EscrowError::InvalidRescueStart.into()));
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
                type S = <TestState as HasTokenVariant>::Token;

                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (order, _) = create_order(test_state).await;

                let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
                let order_ata = S::initialize_spl_associated_account(
                    &mut test_state.context,
                    &token_to_rescue,
                    &order,
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

                let wrong_taker_ata = S::initialize_spl_associated_account(
                    &mut test_state.context,
                    &token_to_rescue,
                    &test_state.maker_wallet.keypair.pubkey(),
                )
                .await;

                let transaction = get_rescue_funds_from_order_tx(
                    test_state,
                    &order,
                    &order_ata,
                    &token_to_rescue,
                    &wrong_taker_ata,
                );

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp + common::constants::RESCUE_DELAY + 100,
                );

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()))
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_with_wrong_hash_order(test_state: &mut TestState) {
                type S = <TestState as HasTokenVariant>::Token;

                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (order, _) = create_order(test_state).await;

                let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
                let order_ata = S::initialize_spl_associated_account(
                    &mut test_state.context,
                    &token_to_rescue,
                    &order,
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

                let taker_ata = S::initialize_spl_associated_account(
                    &mut test_state.context,
                    &token_to_rescue,
                    &test_state.taker_wallet.keypair.pubkey(),
                )
                .await;

                test_state.test_arguments.order_amount += 1; // Change order amount to make hash different
                let transaction = get_rescue_funds_from_order_tx(
                    test_state,
                    &order,
                    &order_ata,
                    &token_to_rescue,
                    &taker_ata,
                );

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp + common::constants::RESCUE_DELAY + 100,
                );

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(ErrorCode::ConstraintSeeds.into()))
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_rescue_funds_from_order_with_wrong_order_ata(
                test_state: &mut TestState,
            ) {
                type S = <TestState as HasTokenVariant>::Token;

                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                let (order, order_ata) = create_order(test_state).await;

                let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
                let taker_ata = S::initialize_spl_associated_account(
                    &mut test_state.context,
                    &token_to_rescue,
                    &test_state.taker_wallet.keypair.pubkey(),
                )
                .await;

                let transaction = get_rescue_funds_from_order_tx(
                    test_state,
                    &order,
                    &order_ata, // Use order ata for order mint, but not for token to rescue
                    &token_to_rescue,
                    &taker_ata,
                );

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp + common::constants::RESCUE_DELAY + 100,
                );

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(ErrorCode::ConstraintAssociated.into()))
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
            async fn test_rescue_tokens_when_escrow_is_deleted(test_state: &mut TestState) {
                create_order(test_state).await;
                prepare_resolvers(
                    test_state,
                    &[
                        test_state.taker_wallet.keypair.pubkey(),
                        test_state.maker_wallet.keypair.pubkey(),
                    ],
                )
                .await;
                common_escrow_tests::test_rescue_tokens_when_escrow_is_deleted(test_state).await;
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
    }
);
