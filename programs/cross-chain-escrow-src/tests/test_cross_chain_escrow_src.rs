use anchor_lang::{error::ErrorCode, prelude::ProgramError};
use anchor_spl::associated_token::{spl_associated_token_account, ID as spl_associated_token_id};
use anchor_spl::token::spl_token::native_mint::ID as NATIVE_MINT;
use anchor_spl::token::spl_token::state::Account as SplTokenAccount;
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
    (TokenSPL, token_spl_tests),
    (Token2022, token_2022_tests) | SrcProgram,
    mod token_module {

        use super::*;

        mod test_order_creation {
            use super::*;
            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation(test_state: &mut TestState) {
                common_escrow_tests::test_escrow_creation(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fail_with_zero_amount(test_state: &mut TestState) {
                common_escrow_tests::test_escrow_creation_fail_with_zero_amount(test_state).await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fail_with_insufficient_funds(test_state: &mut TestState) {
                common_escrow_tests::test_escrow_creation_fail_with_insufficient_funds(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fail_with_insufficient_tokens(test_state: &mut TestState) {
                common_escrow_tests::test_escrow_creation_fail_with_insufficient_tokens(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fail_with_existing_order_hash(test_state: &mut TestState) {
                common_escrow_tests::test_escrow_creation_fail_with_existing_order_hash(test_state)
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fail_if_finality_duration_overflows(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_escrow_creation_fail_if_finality_duration_overflows(
                    test_state,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fail_if_withdrawal_duration_overflows(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_escrow_creation_fail_if_withdrawal_duration_overflows(
                    test_state,
                )
                .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fail_if_public_withdrawal_duration_overflows(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_escrow_creation_fail_if_public_withdrawal_duration_overflows(
                    test_state,
                )
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fail_if_cancellation_duration_overflows(
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

        mod test_escrow_creation {
            use super::*;
            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation(test_state: &mut TestState) {
                let (order, order_ata) = create_escrow(test_state).await;
                let (escrow, escrow_ata) = local_helpers::create_taker_escrow(test_state).await;

                // Check token balances for the escrow account and creator are as expected.
                assert_eq!(
                    DEFAULT_ESCROW_AMOUNT,
                    get_token_balance(&mut test_state.context, &escrow_ata).await
                );

                // Check the lamport balance of escrow account is as expected.
                let escrow_data_len =
                    <SrcProgram as EscrowVariant<TokenSPL>>::get_escrow_data_len();
                let rent_lamports =
                    get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

                assert_eq!(
                    rent_lamports,
                    test_state.client.get_balance(escrow).await.unwrap()
                );

                // Check that orders accounts have been closed.
                let acc_lookup_result = test_state.client.get_account(order).await.unwrap();
                assert!(acc_lookup_result.is_none());

                let acc_lookup_result = test_state.client.get_account(order_ata).await.unwrap();
                assert!(acc_lookup_result.is_none());
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_escrow_creation_fail_with_wrong_token(test_state: &mut TestState) {
                create_escrow(test_state).await;

                test_state.token = solana_sdk::pubkey::Pubkey::new_unique();
                let (escrow, escrow_ata, tx) = local_helpers::create_taker_escrow_data(test_state);

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
            async fn test_escrow_creation_fail_with_empty_order_account(
                test_state: &mut TestState,
            ) {
                // Create an escrow account without existing order account.
                let (escrow, escrow_ata, tx) = local_helpers::create_taker_escrow_data(test_state);

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
        }

        mod test_order_withdraw {
            use super::*;
            #[test_context(TestState)]
            #[tokio::test]
            async fn test_order_creation_fail_if_public_withdrawal_duration_overflows(
                test_state: &mut TestState,
            ) {
                common_escrow_tests::test_escrow_creation_fail_if_public_withdrawal_duration_overflows(
                    test_state,
                )
                    .await
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_only(test_state: &mut TestState) {
                common_escrow_tests::test_withdraw(test_state).await
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
            async fn test_withdraw_does_not_work_with_wrong_order_ata(test_state: &mut TestState) {
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

        mod test_order_public_withdraw {
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
            async fn test_public_withdraw_fails_with_wrong_order_ata(test_state: &mut TestState) {
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

    pub fn get_taker_escrow_addresses<S: TokenVariant>(
        test_state: &TestStateBase<SrcProgram, S>,
        creator: Pubkey,
    ) -> (Pubkey, Pubkey) {
        let (program_id, _) = <SrcProgram as EscrowVariant<TokenSPL>>::get_program_spec();
        let (escrow_pda, _) = Pubkey::find_program_address(
            &[
                b"takerescrow",
                test_state.order_hash.as_ref(),
                test_state.hashlock.as_ref(),
                creator.as_ref(),
                test_state.recipient_wallet.keypair.pubkey().as_ref(),
                test_state.token.as_ref(),
                test_state
                    .test_arguments
                    .escrow_amount
                    .to_be_bytes()
                    .as_ref(),
                test_state
                    .test_arguments
                    .safety_deposit
                    .to_be_bytes()
                    .as_ref(),
                test_state
                    .test_arguments
                    .rescue_start
                    .to_be_bytes()
                    .as_ref(),
            ],
            &program_id,
        );
        let escrow_ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            &escrow_pda,
            &test_state.token,
            &S::get_token_program_id(),
        );

        (escrow_pda, escrow_ata)
    }

    pub fn create_taker_escrow_data<S: TokenVariant>(
        test_state: &TestStateBase<SrcProgram, S>,
    ) -> (Pubkey, Pubkey, Transaction) {
        let (order_pda, order_ata) =
            get_escrow_addresses(test_state, test_state.creator_wallet.keypair.pubkey());
        let (escrow_pda, escrow_ata) =
            get_taker_escrow_addresses(test_state, test_state.creator_wallet.keypair.pubkey());
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_src::instruction::CreateEscrow {});

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_src::id(),
            accounts: vec![
                AccountMeta::new(test_state.recipient_wallet.keypair.pubkey(), true),
                AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), false),
                AccountMeta::new_readonly(test_state.token, false),
                AccountMeta::new(order_pda, false),
                AccountMeta::new(order_ata, false),
                AccountMeta::new(escrow_pda, false),
                AccountMeta::new(escrow_ata, false),
                AccountMeta::new_readonly(spl_associated_token_id, false),
                AccountMeta::new_readonly(S::get_token_program_id(), false),
                AccountMeta::new_readonly(system_program_id, false),
            ],
            data: instruction_data,
        };
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&test_state.payer_kp.pubkey()),
            &[&test_state.payer_kp, &test_state.recipient_wallet.keypair],
            test_state.context.last_blockhash,
        );
        (escrow_pda, escrow_ata, transaction)
    }

    pub async fn create_taker_escrow<S: TokenVariant>(
        test_state: &mut TestStateBase<SrcProgram, S>,
    ) -> (Pubkey, Pubkey) {
        let (escrow, escrow_ata, tx) = create_taker_escrow_data(test_state);
        test_state
            .client
            .process_transaction(tx)
            .await
            .expect_success();
        (escrow, escrow_ata)
    }
}

// Native Mint (wrapped SOL) is always owned by the SPL Token program
type TestState = TestStateBase<SrcProgram, TokenSPL>;

mod test_escrow_native {
    use solana_program_pack::Pack;

    use super::*;
    use crate::local_helpers::create_public_cancel_tx;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        common_escrow_tests::test_escrow_creation_native(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_if_token_is_not_native(test_state: &mut TestState) {
        test_state.test_arguments.asset_is_native = true;
        common_escrow_tests::test_escrow_creation_fail_if_token_is_not_native(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        common_escrow_tests::test_withdraw(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_by_resolver(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        let withdrawer = test_state.recipient_wallet.keypair.insecure_clone();
        common_escrow_tests::test_public_withdraw_tokens(test_state, withdrawer).await
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
        common_escrow_tests::test_public_withdraw_tokens(test_state, withdrawer).await
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

mod test_escrow_wrapped_native {
    use solana_program_pack::Pack;

    use super::*;
    use crate::local_helpers::create_public_cancel_tx;

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
        common_escrow_tests::test_withdraw(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_by_resolver(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        let withdrawer = test_state.recipient_wallet.keypair.insecure_clone();
        common_escrow_tests::test_public_withdraw_tokens(test_state, withdrawer).await
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
        common_escrow_tests::test_public_withdraw_tokens(test_state, withdrawer).await
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
