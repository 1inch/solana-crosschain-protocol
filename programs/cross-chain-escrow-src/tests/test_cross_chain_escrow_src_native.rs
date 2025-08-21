use anchor_lang::prelude::ProgramError;
use anchor_spl::token::spl_token::native_mint::ID as NATIVE_MINT;
use anchor_spl::token::spl_token::state::Account as SplTokenAccount;
use common::{error::EscrowError, timelocks::Stage};
use common_tests::helpers::*;
use common_tests::src_program::{
    create_order, create_order_data, create_public_escrow_cancel_tx,
    get_cancel_order_by_resolver_tx, get_cancel_order_tx, SrcProgram,
};

use common_tests::tests as common_escrow_tests;
use common_tests::whitelist::prepare_resolvers_src;
use solana_program_test::tokio;
use solana_sdk::signature::Signer;
use solana_sdk::signer::keypair::Keypair;
use test_context::test_context;

pub mod helpers_src;
use helpers_src::*;

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
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
            .expect_error(ProgramError::Custom(
                EscrowError::InconsistentNativeTrait.into(),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;
        helpers_src::test_withdraw_escrow(test_state, &escrow, &escrow_ata).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_by_taker(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;
        let withdrawer = test_state.taker_wallet.keypair.insecure_clone();
        prepare_resolvers_src(test_state, &[withdrawer.pubkey()]).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction =
            SrcProgram::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

        set_time(
            &mut test_state.context,
            test_state
                .test_arguments
                .src_timelocks
                .get(Stage::SrcPublicWithdrawal)
                .unwrap(),
        );

        let escrow_data_len = cross_chain_escrow_src::constants::DISCRIMINATOR_BYTES
            + cross_chain_escrow_src::EscrowSrc::INIT_SPACE;

        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;

        test_state
            .expect_state_change(
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
        prepare_resolvers_src(
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
            test_state
                .test_arguments
                .src_timelocks
                .get(Stage::SrcPublicWithdrawal)
                .unwrap(),
        );

        let escrow_data_len = cross_chain_escrow_src::constants::DISCRIMINATOR_BYTES
            + cross_chain_escrow_src::EscrowSrc::INIT_SPACE;

        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;

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
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
            .expect_error(ProgramError::Custom(
                EscrowError::InconsistentNativeTrait.into(),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_for_free_at_the_auction_start(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_cancel_by_resolver_after_auction(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_reward_less_then_auction_calculated(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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

        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        set_time(
            &mut test_state.context,
            test_state.test_arguments.expiration_time,
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
            .expect_error(ProgramError::Custom(
                EscrowError::InconsistentNativeTrait.into(),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;
        let escrow_rent =
            get_min_rent_for_size(&mut test_state.client, DEFAULT_SRC_ESCROW_SIZE).await;

        test_state
            .expect_state_change(
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
                    account_closure(escrow, true),
                    account_closure(escrow_ata, true),
                ],
            )
            .await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_cancel_by_any_account(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;

        let canceller = Keypair::new();
        prepare_resolvers_src(
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
            test_state
                .test_arguments
                .src_timelocks
                .get(Stage::SrcPublicCancellation)
                .unwrap(),
        );

        let escrow_data_len = DEFAULT_SRC_ESCROW_SIZE;
        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, SplTokenAccount::LEN).await;

        test_state
            .expect_state_change(
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
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        common_escrow_tests::test_rescue_all_tokens_and_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_part_of_tokens_and_not_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order(test_state).await;
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        common_escrow_tests::test_rescue_part_of_tokens_and_not_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_all_tokens_from_order_and_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_rescue_all_tokens_from_order_and_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_part_of_tokens_from_order_and_not_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_rescue_part_of_tokens_from_order_and_not_close_ata(test_state).await
    }
}

mod test_wrapped_native {
    use anchor_lang::Space;
    use solana_program_pack::Pack;

    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_order_creation(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_order_creation(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order(test_state).await;
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        common_escrow_tests::test_escrow_creation(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order(test_state).await;
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;
        helpers_src::test_withdraw_escrow(test_state, &escrow, &escrow_ata).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_by_taker(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order(test_state).await;
        let withdrawer = test_state.taker_wallet.keypair.insecure_clone();
        prepare_resolvers_src(test_state, &[withdrawer.pubkey()]).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction =
            SrcProgram::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

        set_time(
            &mut test_state.context,
            test_state
                .test_arguments
                .src_timelocks
                .get(Stage::SrcPublicWithdrawal)
                .unwrap(),
        );

        let escrow_data_len = cross_chain_escrow_src::constants::DISCRIMINATOR_BYTES
            + cross_chain_escrow_src::EscrowSrc::INIT_SPACE;

        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;

        test_state
            .expect_state_change(
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
        prepare_resolvers_src(
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
            test_state
                .test_arguments
                .src_timelocks
                .get(Stage::SrcPublicWithdrawal)
                .unwrap(),
        );

        let escrow_data_len = cross_chain_escrow_src::constants::DISCRIMINATOR_BYTES
            + cross_chain_escrow_src::EscrowSrc::INIT_SPACE;

        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;

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
            .expect_error(ProgramError::Custom(
                EscrowError::InconsistentNativeTrait.into(),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_for_free_at_the_auction_start(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_cancel_by_resolver_for_free_at_the_auction_start(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_at_different_points(test_state: &mut TestState) {
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
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
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_cancel_by_resolver_after_auction(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_reward_less_then_auction_calculated(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        helpers_src::test_cancel_by_resolver_reward_less_then_auction_calculated(test_state).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_by_resolver_fails_if_wrapped_and_maker_ata_not_provided(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        let (order, order_ata) = create_order(test_state).await;

        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        set_time(
            &mut test_state.context,
            test_state.test_arguments.expiration_time,
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
            .expect_error(ProgramError::Custom(
                EscrowError::InconsistentNativeTrait.into(),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order(test_state).await;
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        let (escrow, escrow_ata) = create_escrow(test_state).await;
        common_escrow_tests::test_cancel(test_state, &escrow, &escrow_ata).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_cancel_by_any_account(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;

        create_order(test_state).await;

        let canceller = Keypair::new();
        prepare_resolvers_src(
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
            test_state
                .test_arguments
                .src_timelocks
                .get(Stage::SrcPublicCancellation)
                .unwrap(),
        );

        let escrow_data_len = DEFAULT_SRC_ESCROW_SIZE;
        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, SplTokenAccount::LEN).await;

        test_state
            .expect_state_change(
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
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        common_escrow_tests::test_rescue_all_tokens_and_close_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_rescue_part_of_tokens_and_not_close_ata(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order(test_state).await;
        prepare_resolvers_src(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        common_escrow_tests::test_rescue_part_of_tokens_and_not_close_ata(test_state).await
    }
}
