use anchor_lang::{error::ErrorCode, prelude::ProgramError};
use anchor_spl::token::spl_token::native_mint::ID as NATIVE_MINT;
use anchor_spl::token::spl_token::state::Account as SplTokenAccount;
use common::error::EscrowError;
use common_tests::helpers::*;
use common_tests::run_for_tokens;
use common_tests::src_program::{create_order, create_order_data, get_order_data_len, SrcProgram};
use common_tests::tests as common_escrow_tests;
use solana_program_test::tokio;
use solana_sdk::signature::Signer;
use solana_sdk::signer::keypair::Keypair;
use test_context::test_context;

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
                test_state.test_arguments.escrow_amount = 0;
                let (order, order_ata, _) = create_order_data(test_state);

                let transaction =
                    common_tests::src_program::get_create_order_tx(test_state, &order, &order_ata);

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
                let (order, order_ata, _) = create_order_data(test_state);

                let transaction =
                    common_tests::src_program::get_create_order_tx(test_state, &order, &order_ata);

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

                let (order, order_ata, _) = create_order_data(test_state);

                let transaction =
                    common_tests::src_program::get_create_order_tx(test_state, &order, &order_ata);

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
        }

        mod test_escrow_withdraw {
            use super::*;
            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_only(test_state: &mut TestState) {
                create_order(test_state).await;
                let rent_recipient = test_state.recipient_wallet.keypair.pubkey();
                common_escrow_tests::test_withdraw(
                    test_state,
                    rent_recipient,
                )
                .await
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
                let new_amount = test_state.test_arguments.escrow_amount + diff_amount;
                test_state.test_arguments.escrow_amount = new_amount;
                create_order(test_state).await;
                test_state.test_arguments.escrow_amount -= diff_amount;
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
                let new_amount = test_state.test_arguments.escrow_amount + diff;
                test_state.test_arguments.escrow_amount = new_amount;
                create_order(test_state).await;
                test_state.test_arguments.escrow_amount -= diff;
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

        mod test_order_cancel {
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
            async fn test_cannot_cancel_with_wrong_order_ata(test_state: &mut TestState) {
                common_escrow_tests::test_cannot_cancel_with_wrong_escrow_ata(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_cancel_before_cancellation_start(test_state: &mut TestState) {
                common_escrow_tests::test_cannot_cancel_before_cancellation_start(test_state).await
            }
        }

        mod test_order_public_cancel {
            use super::local_helpers::*;
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_public_cancel_before_public_cancellation_start(
                test_state: &mut TestState,
            ) {
                let (order, order_ata) = create_escrow(test_state).await;
                let transaction =
                    create_public_cancel_tx(test_state, &order, &order_ata, &test_state.payer_kp);

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

        mod test_order_rescue_funds {
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
            async fn test_cannot_rescue_funds_with_wrong_order_ata(test_state: &mut TestState) {
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
    use solana_program::instruction::{AccountMeta, Instruction};
    use solana_program::pubkey::Pubkey;
    use solana_program::system_program::ID as system_program_id;
    use solana_sdk::signature::Signer;
    use solana_sdk::transaction::Transaction;

    /// Byte offset in the escrow account data where the `dst_amount` field is located
    const DST_AMOUNT_OFFSET: usize = 205;
    const U64_SIZE: usize = size_of::<u64>();

    pub fn create_public_cancel_tx<S: TokenVariant>(
        test_state: &TestStateBase<SrcProgram, S>,
        order: &Pubkey,
        order_ata: &Pubkey,
        canceller: &Keypair,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_src::instruction::PublicCancel {});

        let (creator_ata, _) = find_user_ata(test_state);

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_src::id(),
            accounts: vec![
                AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), false),
                AccountMeta::new_readonly(test_state.token, false),
                AccountMeta::new(canceller.pubkey(), true),
                AccountMeta::new(*order, false),
                AccountMeta::new(*order_ata, false),
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
            test_state.test_arguments.escrow_amount
        } else {
            0
        };

        assert_eq!(
            order_ata_lamports,
            test_state.client.get_balance(order_ata).await.unwrap() - wrapped_sol
        );
    }
}

// Native Mint (wrapped SOL) is always owned by the SPL Token program
type TestState = TestStateBase<SrcProgram, TokenSPL>;

mod test_native_src {
    use solana_program_pack::Pack;

    use super::*;
    use crate::local_helpers::create_public_cancel_tx;

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
        common_escrow_tests::test_withdraw(test_state, rent_recipient)
            .await
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
    async fn test_cancel(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        common_escrow_tests::test_cancel_native(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_cancel_by_creator(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;

        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction = create_public_cancel_tx(
            test_state,
            &escrow,
            &escrow_ata,
            &test_state.creator_wallet.keypair,
        );

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
                &[native_change(
                    test_state.creator_wallet.keypair.pubkey(),
                    rent_lamports + token_account_rent + test_state.test_arguments.escrow_amount,
                )],
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
    async fn test_public_cancel_by_any_account(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;

        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let canceller = Keypair::new();
        transfer_lamports(
            &mut test_state.context,
            WALLET_DEFAULT_LAMPORTS,
            &test_state.payer_kp,
            &canceller.pubkey(),
        )
        .await;

        let transaction = create_public_cancel_tx(test_state, &escrow, &escrow_ata, &canceller);

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
                        rent_lamports + token_account_rent
                            - test_state.test_arguments.safety_deposit
                            + test_state.test_arguments.escrow_amount,
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

mod test_wrapped_native {
    use solana_program_pack::Pack;

    use super::*;
    use crate::local_helpers::create_public_cancel_tx;

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
        common_escrow_tests::test_withdraw(test_state, rent_recipient)
            .await
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
    async fn test_cancel(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        common_escrow_tests::test_cancel(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_cancel_by_creator(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;

        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction = create_public_cancel_tx(
            test_state,
            &escrow,
            &escrow_ata,
            &test_state.creator_wallet.keypair,
        );

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
                    native_change(
                        test_state.creator_wallet.keypair.pubkey(),
                        rent_lamports + token_account_rent,
                    ),
                    token_change(
                        test_state.creator_wallet.native_token_account,
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
    async fn test_public_cancel_by_any_account(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;

        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let canceller = Keypair::new();
        transfer_lamports(
            &mut test_state.context,
            WALLET_DEFAULT_LAMPORTS,
            &test_state.payer_kp,
            &canceller.pubkey(),
        )
        .await;

        let transaction = create_public_cancel_tx(test_state, &escrow, &escrow_ata, &canceller);

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
                        rent_lamports + token_account_rent
                            - test_state.test_arguments.safety_deposit,
                    ),
                    token_change(
                        test_state.creator_wallet.native_token_account,
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
