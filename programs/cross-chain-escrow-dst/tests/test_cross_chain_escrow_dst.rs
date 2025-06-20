use anchor_lang::error::ErrorCode;
use common::error::EscrowError;
use common_tests::dst_program::DstProgram;
use common_tests::helpers::*;
use common_tests::run_for_tokens;
use common_tests::tests as common_escrow_tests;
use common_tests::whitelist::prepare_resolvers;
use solana_program::program_error::ProgramError;
use solana_program_test::tokio;
use solana_sdk::{signature::Signer, signer::keypair::Keypair, sysvar::clock::Clock};
use std::marker::PhantomData;

use test_context::test_context;

use crate::local_helpers::get_token_account_len;

run_for_tokens!(
    (TokenSPL, token_spl_tests),
    (Token2022, token_2022_tests) | DstProgram,
    mod token_module {

        use super::*;

        mod test_escrow_creation {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_only(test_state: &mut TestState) {
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
            async fn test_escrow_creation_fails_if_withdrawal_duration_overflows(
                test_state: &mut TestState,
            ) {
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
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                common_escrow_tests::test_escrow_creation_fails_if_public_withdrawal_duration_overflows(
                    test_state,
                ).await;
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
                let rent_recipient = test_state.maker_wallet.keypair.pubkey();
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                let transaction = DstProgram::get_withdraw_tx(test_state, &escrow, &escrow_ata);

                let token_account_rent = get_min_rent_for_size(
                    &mut test_state.client,
                    get_token_account_len(PhantomData::<TestState>),
                )
                .await;

                let escrow_rent =
                    get_min_rent_for_size(&mut test_state.client, DEFAULT_DST_ESCROW_SIZE).await;

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
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                let transaction = DstProgram::get_withdraw_tx(test_state, &escrow, &escrow_ata);

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
                );

                let (_, taker_ata) = find_user_ata(test_state);

                let excess_amount = 1000;
                // Send excess tokens to the escrow account
                local_helpers::mint_excess_tokens(test_state, &escrow_ata, excess_amount).await;
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
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
                );

                let escrow_data_len = DEFAULT_DST_ESCROW_SIZE;

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
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
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
                    get_token_account_len(PhantomData::<TestState>),
                )
                .await;

                assert_eq!(
                    rent_lamports,
                    test_state.client.get_balance(escrow).await.unwrap()
                );

                let (_, taker_ata) = find_user_ata(test_state);

                test_state
                    .expect_balance_change(
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
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
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
                common_escrow_tests::test_cancel(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_with_excess_tokens(test_state: &mut TestState) {
                let (escrow, escrow_ata) = create_escrow(test_state).await;
                let transaction = DstProgram::get_cancel_tx(test_state, &escrow, &escrow_ata);

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
                );

                let (maker_ata, _) = find_user_ata(test_state);

                let excess_amount = 1000;
                // Send excess tokens to the escrow account
                local_helpers::mint_excess_tokens(test_state, &escrow_ata, excess_amount).await;

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

// Native Mint (wrapped SOL) is always owned by the SPL Token program
type TestState = TestStateBase<DstProgram, TokenSPL>;
// Tests for native token (SOL)
mod test_escrow_native {
    use anchor_lang::Space;
    use anchor_spl::token::spl_token::native_mint::ID as NATIVE_MINT;

    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        // Check the lamport balance of escrow account is as expected.
        let escrow_data_len = cross_chain_escrow_dst::constants::DISCRIMINATOR_BYTES
            + cross_chain_escrow_dst::EscrowDst::INIT_SPACE;
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

        // Check native balance for the maker is as expected.
        assert_eq!(
            WALLET_DEFAULT_LAMPORTS - DEFAULT_ESCROW_AMOUNT - token_account_rent - rent_lamports,
            // The pure lamport balance of the maker wallet after the transaction.
            test_state
                .client
                .get_balance(test_state.maker_wallet.keypair.pubkey())
                .await
                .unwrap()
        );
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
        let (escrow, escrow_ata) = create_escrow(test_state).await;
        let transaction = DstProgram::get_withdraw_tx(test_state, &escrow, &escrow_ata);

        let token_account_rent = get_min_rent_for_size(
            &mut test_state.client,
            get_token_account_len(PhantomData::<TestState>),
        )
        .await;

        let escrow_rent =
            get_min_rent_for_size(&mut test_state.client, DEFAULT_DST_ESCROW_SIZE).await;

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
        );

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(
                        test_state.maker_wallet.keypair.pubkey(),
                        token_account_rent + escrow_rent,
                    ),
                    native_change(
                        test_state.taker_wallet.keypair.pubkey(),
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
    async fn test_public_withdraw_by_maker(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        let withdrawer = test_state.maker_wallet.keypair.insecure_clone();
        prepare_resolvers(test_state, &[withdrawer.pubkey()]).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction =
            DstProgram::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp
                + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
        );

        let escrow_data_len = cross_chain_escrow_dst::constants::DISCRIMINATOR_BYTES
            + cross_chain_escrow_dst::EscrowDst::INIT_SPACE;

        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(
                        test_state.maker_wallet.keypair.pubkey(),
                        rent_lamports + token_account_rent,
                    ),
                    native_change(
                        test_state.taker_wallet.keypair.pubkey(),
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
        let withdrawer = Keypair::new();
        prepare_resolvers(test_state, &[withdrawer.pubkey()]).await;
        let payer_kp = &test_state.payer_kp;
        {
            let context = &mut test_state.context;

            transfer_lamports(
                context,
                WALLET_DEFAULT_LAMPORTS,
                payer_kp,
                &withdrawer.pubkey(),
            )
            .await;
        }
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction =
            DstProgram::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp
                + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
        );

        let escrow_data_len = cross_chain_escrow_dst::constants::DISCRIMINATOR_BYTES
            + cross_chain_escrow_dst::EscrowDst::INIT_SPACE;

        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(
                        test_state.maker_wallet.keypair.pubkey(),
                        rent_lamports + token_account_rent
                            - test_state.test_arguments.safety_deposit,
                    ),
                    native_change(
                        test_state.taker_wallet.keypair.pubkey(),
                        test_state.test_arguments.escrow_amount,
                    ),
                    native_change(
                        withdrawer.pubkey(),
                        test_state.test_arguments.safety_deposit,
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
    async fn test_cancel(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        let (escrow, escrow_ata) = create_escrow(test_state).await;
        let transaction = DstProgram::get_cancel_tx(test_state, &escrow, &escrow_ata);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
        );

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;
        let escrow_rent =
            get_min_rent_for_size(&mut test_state.client, DEFAULT_DST_ESCROW_SIZE).await;

        test_state
            .expect_balance_change(
                transaction,
                &[native_change(
                    test_state.maker_wallet.keypair.pubkey(),
                    test_state.test_arguments.escrow_amount + escrow_rent + token_account_rent,
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
    async fn test_rescue_all_tokens_and_close_ata(test_state: &mut TestState) {
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        common_escrow_tests::test_rescue_all_tokens_and_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_part_of_tokens_and_not_close_ata(test_state: &mut TestState) {
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
        let (escrow, escrow_ata) = create_escrow(test_state).await;
        let transaction = DstProgram::get_withdraw_tx(test_state, &escrow, &escrow_ata);

        let token_account_rent = get_min_rent_for_size(
            &mut test_state.client,
            get_token_account_len(PhantomData::<TestState>),
        )
        .await;

        let escrow_rent =
            get_min_rent_for_size(&mut test_state.client, DEFAULT_DST_ESCROW_SIZE).await;

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
        );

        let (_, taker_ata) = find_user_ata(test_state);

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(
                        test_state.maker_wallet.keypair.pubkey(),
                        token_account_rent + escrow_rent,
                    ),
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
    async fn test_withdraw_fails_with_no_taker_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;

        let (escrow, escrow_ata) = create_escrow(test_state).await;

        test_state.taker_wallet.native_token_account = cross_chain_escrow_dst::ID_CONST;

        let transaction = DstProgram::get_withdraw_tx(test_state, &escrow, &escrow_ata);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
        );

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                0,
                ProgramError::Custom(EscrowError::MissingRecipientAta.into()),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_by_maker(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        let withdrawer = test_state.maker_wallet.keypair.insecure_clone();
        prepare_resolvers(test_state, &[withdrawer.pubkey()]).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction =
            DstProgram::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp
                + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
        );

        let escrow_data_len = DEFAULT_DST_ESCROW_SIZE;

        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

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
                        test_state.maker_wallet.keypair.pubkey(),
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
        let withdrawer = Keypair::new();
        transfer_lamports(
            &mut test_state.context,
            WALLET_DEFAULT_LAMPORTS,
            &test_state.payer_kp,
            &withdrawer.pubkey(),
        )
        .await;
        prepare_resolvers(test_state, &[withdrawer.pubkey()]).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction =
            DstProgram::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp
                + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
        );

        let escrow_data_len = DEFAULT_DST_ESCROW_SIZE;

        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

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
                        test_state.maker_wallet.keypair.pubkey(),
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
    async fn test_public_withdraw_fails_with_no_taker_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        let withdrawer = Keypair::new();
        prepare_resolvers(test_state, &[withdrawer.pubkey()]).await;
        let payer_kp = &test_state.payer_kp;
        let context = &mut test_state.context;

        transfer_lamports(
            context,
            WALLET_DEFAULT_LAMPORTS,
            payer_kp,
            &withdrawer.pubkey(),
        )
        .await;

        let (escrow, escrow_ata) = create_escrow(test_state).await;

        test_state.taker_wallet.native_token_account = cross_chain_escrow_dst::ID_CONST;

        let transaction =
            DstProgram::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp
                + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
        );

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                0,
                ProgramError::Custom(EscrowError::MissingRecipientAta.into()),
            ));
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
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        test_state.token = NATIVE_MINT;
        common_escrow_tests::test_rescue_all_tokens_and_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_part_of_tokens_and_not_close_ata(test_state: &mut TestState) {
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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

mod local_helpers {

    use super::*;
    use solana_program::pubkey::Pubkey;

    pub async fn mint_excess_tokens<S: TokenVariant>(
        test_state: &mut TestStateBase<DstProgram, S>,
        escrow_ata: &Pubkey,
        excess_amount: u64,
    ) {
        S::mint_spl_tokens(
            &mut test_state.context,
            &test_state.token,
            escrow_ata,
            &test_state.payer_kp.pubkey(),
            &test_state.payer_kp,
            excess_amount,
        )
        .await;
    }

    pub fn get_token_account_len<S: TokenVariant>(
        _: PhantomData<TestStateBase<DstProgram, S>>,
    ) -> usize {
        S::get_token_account_size()
    }

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
}
