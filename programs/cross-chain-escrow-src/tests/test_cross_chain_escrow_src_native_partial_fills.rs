use anchor_spl::token::spl_token::native_mint::ID as NATIVE_MINT;
use common_tests::helpers::*;
use common_tests::src_program::SrcProgram;

use common_tests::whitelist::prepare_resolvers;
use solana_program_test::tokio;
use solana_sdk::signature::Signer;
use solana_sdk::signer::keypair::Keypair;
use test_context::test_context;

pub mod helpers_src;
use helpers_src::*;

// Native Mint (wrapped SOL) is always owned by the SPL Token program
type TestState = TestStateBase<SrcProgram, TokenSPL>;

mod test_native_src {

    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_create_escrow_with_merkle_proof_and_leaf_validation(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
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
    async fn test_withdraw_from_partial_order(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

        let secret_index = get_index_for_escrow_amount(test_state, escrow_amount);

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        test_state.secret = test_state.test_arguments.partial_secrets[secret_index];
        helpers_src::test_withdraw_escrow(test_state, &escrow, &escrow_ata).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw_two_escrows_from_partial_order(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

        let secret_index = get_index_for_escrow_amount(test_state, escrow_amount);

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        let secret_index_2 = get_index_for_escrow_amount(test_state, escrow_amount);

        let (escrow_2, escrow_ata_2) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        test_state.secret = test_state.test_arguments.partial_secrets[secret_index];
        helpers_src::test_withdraw_escrow(test_state, &escrow, &escrow_ata).await;

        test_state.secret = test_state.test_arguments.partial_secrets[secret_index_2];
        helpers_src::test_withdraw_escrow(test_state, &escrow_2, &escrow_ata_2).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_from_partial_order_by_taker(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

        let secret_index = get_index_for_escrow_amount(test_state, escrow_amount);

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        test_state.secret = test_state.test_arguments.partial_secrets[secret_index];

        let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

        helpers_src::test_public_withdraw_escrow(test_state, &escrow, &escrow_ata, &taker_kp).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_two_escrows_from_partial_order_by_taker(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

        let secret_index = get_index_for_escrow_amount(test_state, escrow_amount);

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        let secret_index_2 = get_index_for_escrow_amount(test_state, escrow_amount);

        let (escrow_2, escrow_ata_2) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

        test_state.secret = test_state.test_arguments.partial_secrets[secret_index];

        helpers_src::test_public_withdraw_escrow(test_state, &escrow, &escrow_ata, &taker_kp).await;

        test_state.secret = test_state.test_arguments.partial_secrets[secret_index_2];

        helpers_src::test_public_withdraw_escrow(test_state, &escrow_2, &escrow_ata_2, &taker_kp)
            .await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_from_partial_order_by_any_resolver(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order_for_partial_fill(test_state).await;

        let withdrawer = Keypair::new();

        transfer_lamports(
            &mut test_state.context,
            WALLET_DEFAULT_LAMPORTS,
            &test_state.payer_kp,
            &withdrawer.pubkey(),
        )
        .await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
        prepare_resolvers(
            test_state,
            &[
                test_state.taker_wallet.keypair.pubkey(),
                withdrawer.pubkey(),
            ],
        )
        .await;

        let secret_index = get_index_for_escrow_amount(test_state, escrow_amount);

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        test_state.secret = test_state.test_arguments.partial_secrets[secret_index];

        helpers_src::test_public_withdraw_escrow(test_state, &escrow, &escrow_ata, &withdrawer)
            .await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_escrow_from_partial_order(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        test_cancel_escrow_partial_native(test_state, &escrow, &escrow_ata).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_two_escrows_from_partial_order(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        test_state.test_arguments.escrow_amount = escrow_amount;
        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        let (escrow_2, escrow_ata_2) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        test_cancel_escrow_partial_native(test_state, &escrow, &escrow_ata).await;
        test_cancel_escrow_partial_native(test_state, &escrow_2, &escrow_ata_2).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_cancel_escrow_from_partial_order_by_taker(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

        test_public_cancel_escrow_native(test_state, &escrow, &escrow_ata, &taker_kp).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_cancel_two_escrows_from_partial_order_by_taker(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        let (escrow_2, escrow_ata_2) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

        test_public_cancel_escrow_native(test_state, &escrow, &escrow_ata, &taker_kp).await;
        test_public_cancel_escrow_native(test_state, &escrow_2, &escrow_ata_2, &taker_kp).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_cancel_escrow_from_partial_order_by_any_resolver(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        test_state.test_arguments.asset_is_native = true;
        create_order_for_partial_fill(test_state).await;

        let canceller = Keypair::new();

        transfer_lamports(
            &mut test_state.context,
            WALLET_DEFAULT_LAMPORTS,
            &test_state.payer_kp,
            &canceller.pubkey(),
        )
        .await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
        prepare_resolvers(
            test_state,
            &[test_state.taker_wallet.keypair.pubkey(), canceller.pubkey()],
        )
        .await;

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        test_public_cancel_escrow_native(test_state, &escrow, &escrow_ata, &canceller).await;
    }
}

mod test_wrapped_native {

    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_create_escrow_with_merkle_proof_and_leaf_validation(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
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
    async fn test_withdraw_from_partial_order(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

        let secret_index = get_index_for_escrow_amount(test_state, escrow_amount);

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        test_state.secret = test_state.test_arguments.partial_secrets[secret_index];
        helpers_src::test_withdraw_escrow(test_state, &escrow, &escrow_ata).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw_two_escrows_from_partial_order(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

        let secret_index = get_index_for_escrow_amount(test_state, escrow_amount);

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        let secret_index_2 = get_index_for_escrow_amount(test_state, escrow_amount);

        let (escrow_2, escrow_ata_2) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        test_state.secret = test_state.test_arguments.partial_secrets[secret_index];
        helpers_src::test_withdraw_escrow(test_state, &escrow, &escrow_ata).await;

        test_state.secret = test_state.test_arguments.partial_secrets[secret_index_2];
        helpers_src::test_withdraw_escrow(test_state, &escrow_2, &escrow_ata_2).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_from_partial_order_by_taker(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

        let secret_index = get_index_for_escrow_amount(test_state, escrow_amount);

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        test_state.secret = test_state.test_arguments.partial_secrets[secret_index];

        let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

        helpers_src::test_public_withdraw_escrow(test_state, &escrow, &escrow_ata, &taker_kp).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_two_escrows_from_partial_order_by_taker(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

        let secret_index = get_index_for_escrow_amount(test_state, escrow_amount);

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        let secret_index_2 = get_index_for_escrow_amount(test_state, escrow_amount);

        let (escrow_2, escrow_ata_2) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

        test_state.secret = test_state.test_arguments.partial_secrets[secret_index];

        helpers_src::test_public_withdraw_escrow(test_state, &escrow, &escrow_ata, &taker_kp).await;

        test_state.secret = test_state.test_arguments.partial_secrets[secret_index_2];

        helpers_src::test_public_withdraw_escrow(test_state, &escrow_2, &escrow_ata_2, &taker_kp)
            .await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_from_partial_order_by_any_resolver(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order_for_partial_fill(test_state).await;

        let withdrawer = Keypair::new();

        transfer_lamports(
            &mut test_state.context,
            WALLET_DEFAULT_LAMPORTS,
            &test_state.payer_kp,
            &withdrawer.pubkey(),
        )
        .await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
        prepare_resolvers(
            test_state,
            &[
                test_state.taker_wallet.keypair.pubkey(),
                withdrawer.pubkey(),
            ],
        )
        .await;

        let secret_index = get_index_for_escrow_amount(test_state, escrow_amount);

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        test_state.secret = test_state.test_arguments.partial_secrets[secret_index];

        helpers_src::test_public_withdraw_escrow(test_state, &escrow, &escrow_ata, &withdrawer)
            .await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_escrow_from_partial_order(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        test_cancel_escrow_partial(test_state, &escrow, &escrow_ata).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cancel_two_escrows_from_partial_order(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
        test_state.test_arguments.escrow_amount = escrow_amount;
        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        let (escrow_2, escrow_ata_2) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        test_cancel_escrow_partial(test_state, &escrow, &escrow_ata).await;
        test_cancel_escrow_partial(test_state, &escrow_2, &escrow_ata_2).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_cancel_escrow_from_partial_order_by_taker(test_state: &mut TestState) {
        test_state.token = NATIVE_MINT;
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

        test_public_cancel_escrow(test_state, &escrow, &escrow_ata, &taker_kp).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_cancel_two_escrows_from_partial_order_by_taker(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        create_order_for_partial_fill(test_state).await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
        prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        let (escrow_2, escrow_ata_2) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

        test_public_cancel_escrow(test_state, &escrow, &escrow_ata, &taker_kp).await;
        test_public_cancel_escrow(test_state, &escrow_2, &escrow_ata_2, &taker_kp).await;
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_cancel_escrow_from_partial_order_by_any_resolver(
        test_state: &mut TestState,
    ) {
        test_state.token = NATIVE_MINT;
        create_order_for_partial_fill(test_state).await;

        let canceller = Keypair::new();

        transfer_lamports(
            &mut test_state.context,
            WALLET_DEFAULT_LAMPORTS,
            &test_state.payer_kp,
            &canceller.pubkey(),
        )
        .await;

        let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
        prepare_resolvers(
            test_state,
            &[test_state.taker_wallet.keypair.pubkey(), canceller.pubkey()],
        )
        .await;

        let (escrow, escrow_ata) =
            test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

        test_public_cancel_escrow(test_state, &escrow, &escrow_ata, &canceller).await;
    }
}
