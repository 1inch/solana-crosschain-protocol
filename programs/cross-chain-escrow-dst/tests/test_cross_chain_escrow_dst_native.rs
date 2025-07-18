use anchor_spl::token::spl_token::native_mint::ID as NATIVE_MINT;
use common::{error::EscrowError, timelocks::Stage};
use common_tests::dst_program::DstProgram;
use common_tests::helpers::*;
use common_tests::tests as common_escrow_tests;
use common_tests::whitelist::prepare_resolvers_dst;
use solana_program::program_error::ProgramError;
use solana_program_test::tokio;
use solana_sdk::{signature::Signer, signer::keypair::Keypair};
use test_context::test_context;

// Native Mint (wrapped SOL) is always owned by the SPL Token program
type TestState = TestStateBase<DstProgram, TokenSPL>;
// Tests for native token (SOL)
mod test_escrow_native {
    use anchor_lang::Space;

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

        // Check native balance for the creator is as expected.
        assert_eq!(
            WALLET_DEFAULT_LAMPORTS - DEFAULT_ESCROW_AMOUNT - token_account_rent - rent_lamports,
            // The pure lamport balance of the creator wallet after the transaction.
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

        test_state
            .expect_state_change(
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
        prepare_resolvers_dst(test_state, &[withdrawer.pubkey()]).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction =
            DstProgram::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

        set_time(
            &mut test_state.context,
            test_state
                .test_arguments
                .dst_timelocks
                .get(Stage::DstPublicWithdrawal)
                .unwrap(),
        );

        let escrow_data_len = cross_chain_escrow_dst::constants::DISCRIMINATOR_BYTES
            + cross_chain_escrow_dst::EscrowDst::INIT_SPACE;

        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;

        test_state
            .expect_state_change(
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
        prepare_resolvers_dst(test_state, &[withdrawer.pubkey()]).await;
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
            test_state
                .test_arguments
                .dst_timelocks
                .get(Stage::DstPublicWithdrawal)
                .unwrap(),
        );

        let escrow_data_len = cross_chain_escrow_dst::constants::DISCRIMINATOR_BYTES
            + cross_chain_escrow_dst::EscrowDst::INIT_SPACE;

        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;

        test_state
            .expect_state_change(
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
            test_state
                .test_arguments
                .dst_timelocks
                .get(Stage::DstCancellation)
                .unwrap(),
        );

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;
        let escrow_rent =
            get_min_rent_for_size(&mut test_state.client, DEFAULT_DST_ESCROW_SIZE).await;

        test_state
            .expect_state_change(
                transaction,
                &[
                    native_change(
                        test_state.maker_wallet.keypair.pubkey(),
                        test_state.test_arguments.escrow_amount + escrow_rent + token_account_rent,
                    ),
                    account_closure(escrow_ata, true),
                    account_closure(escrow, true),
                ],
            )
            .await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_all_tokens_and_close_ata(test_state: &mut TestState) {
        prepare_resolvers_dst(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        common_escrow_tests::test_rescue_all_tokens_and_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_part_of_tokens_and_not_close_ata(test_state: &mut TestState) {
        prepare_resolvers_dst(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
            test_state
                .test_arguments
                .dst_timelocks
                .get(Stage::DstWithdrawal)
                .unwrap(),
        );

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error(ProgramError::Custom(
                EscrowError::MissingRecipientAta.into(),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_by_maker(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        let withdrawer = test_state.maker_wallet.keypair.insecure_clone();
        prepare_resolvers_dst(test_state, &[withdrawer.pubkey()]).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction =
            DstProgram::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

        set_time(
            &mut test_state.context,
            test_state
                .test_arguments
                .dst_timelocks
                .get(Stage::DstPublicWithdrawal)
                .unwrap(),
        );

        let escrow_data_len = DEFAULT_DST_ESCROW_SIZE;

        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

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
        prepare_resolvers_dst(test_state, &[withdrawer.pubkey()]).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction =
            DstProgram::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

        set_time(
            &mut test_state.context,
            test_state
                .test_arguments
                .dst_timelocks
                .get(Stage::DstPublicWithdrawal)
                .unwrap(),
        );

        let escrow_data_len = DEFAULT_DST_ESCROW_SIZE;

        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

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
        prepare_resolvers_dst(test_state, &[withdrawer.pubkey()]).await;
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
                EscrowError::MissingRecipientAta.into(),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        let (escrow, escrow_ata) = create_escrow(test_state).await;
        common_escrow_tests::test_cancel(test_state, &escrow, &escrow_ata).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_all_tokens_and_close_ata(test_state: &mut TestState) {
        prepare_resolvers_dst(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        test_state.token = NATIVE_MINT;
        common_escrow_tests::test_rescue_all_tokens_and_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_part_of_tokens_and_not_close_ata(test_state: &mut TestState) {
        prepare_resolvers_dst(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        test_state.token = NATIVE_MINT;
        common_escrow_tests::test_rescue_part_of_tokens_and_not_close_ata(test_state).await
    }
}
