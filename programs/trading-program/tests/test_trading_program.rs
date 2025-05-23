use anchor_lang::error::ErrorCode;
use common_tests::{helpers::*, src_program::SrcProgram};

use solana_program::program_error::ProgramError;
use solana_program_test::tokio;
use solana_sdk::signature::Signer;
use test_context::test_context;
use trading_program::utils::error::TradingProgramError;
mod utils;
use common_tests::trading_program_run_for_tokens;
use solana_sdk::signer::keypair::Keypair;
use utils::{
    create_escrow_via_trading_program, create_signinig_default_order_ix, init_escrow_src_tx,
    prepare_trading_account, TestStateTrading,
};

pub async fn deploy_spl_token_<T, S: TokenVariant>(t: &mut TestStateBase<T, S>) -> Keypair {
    S::deploy_spl_token(&mut t.context).await
}

trading_program_run_for_tokens!(
    (Token2020, token_2020_tests),
    (Token2022, token_2022_tests) |
mod test_trading_program {
    use common_tests::helpers::Expectation;

    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_via_trading_program(test_state_trading: &mut TestState) {
        let test_state = &mut test_state_trading.base;

        let (escrow, escrow_ata, _, trading_ata) =
            create_escrow_via_trading_program(test_state).await;

        // Check token balances for the escrow account and creator are as expected.
        assert_eq!(
            get_token_balance(&mut test_state.context, &escrow_ata).await,
            DEFAULT_ESCROW_AMOUNT
        );
        assert_eq!(
            get_token_balance(&mut test_state.context, &trading_ata).await,
            0
        );
        // Check the lamport balance of escrow account is as expected.
        let rent_lamports =
            get_min_rent_for_size(&mut test_state.client, <SrcProgram as EscrowVariant<Token2020>>::get_escrow_data_len()).await;
        assert_eq!(
            rent_lamports,
            test_state.client.get_balance(escrow).await.unwrap()
        );
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_via_trading_program_fail_with_wrong_signer(
        test_state_trading: &mut TestState,
    ) {
        let test_state = &mut test_state_trading.base;
        let (escrow_pda, escrow_ata, trading_pda, trading_ata) =
            prepare_trading_account(test_state).await;

        let instruction0 = create_signinig_default_order_ix(
            test_state,
            test_state.recipient_wallet.keypair.insecure_clone(), // Wrong signer
        );

        let transaction = init_escrow_src_tx(
            test_state,
            escrow_pda,
            escrow_ata,
            trading_pda,
            trading_ata,
            instruction0,
        );

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                1,
                ProgramError::Custom(TradingProgramError::SigVerificationFailed.into()),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_via_trading_program_fail_with_wrong_maker(
        test_state_trading: &mut TestState,
    ) {
        let test_state = &mut test_state_trading.base;

        let instruction0 = create_signinig_default_order_ix(
            test_state,
            test_state.creator_wallet.keypair.insecure_clone(),
        );

        test_state.creator_wallet = test_state.recipient_wallet.clone(); // Set wrong wallet to compute and use wrong accounts
        let (escrow_pda, escrow_ata, trading_pda, trading_ata) =
            prepare_trading_account(test_state).await;

        let transaction = init_escrow_src_tx(
            test_state,
            escrow_pda,
            escrow_ata,
            trading_pda,
            trading_ata,
            instruction0,
        );

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((1, ProgramError::Custom(ErrorCode::ConstraintSeeds.into())));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_via_trading_program_fail_with_wrong_token(
        test_state_trading: &mut TestState,
    ) {
        let test_state = &mut test_state_trading.base;

        let instruction0 = create_signinig_default_order_ix(
            test_state,
            test_state.creator_wallet.keypair.insecure_clone(),
        );

        test_state.token = deploy_spl_token_(test_state).await.pubkey(); // Wrong token
        let (escrow_pda, escrow_ata, trading_pda, trading_ata) =
            prepare_trading_account(test_state).await;

        let transaction = init_escrow_src_tx(
            test_state,
            escrow_pda,
            escrow_ata,
            trading_pda,
            trading_ata,
            instruction0,
        );

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                1,
                ProgramError::Custom(TradingProgramError::OrderDataMismatch.into()),
            ));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_via_trading_program_fail_with_wrong_trading_account_ata_seed(
        test_state_trading: &mut TestState,
    ) {
        let test_state = &mut test_state_trading.base;

        let (escrow_pda, escrow_ata, trading_pda, trading_ata) =
            prepare_trading_account(test_state).await;

        let instruction0 = create_signinig_default_order_ix(
            test_state,
            test_state.creator_wallet.keypair.insecure_clone(),
        );

        test_state.token = deploy_spl_token_(test_state).await.pubkey(); // Wrong derivation of the trading_account_ata
        let transaction = init_escrow_src_tx(
            test_state,
            escrow_pda,
            escrow_ata,
            trading_pda,
            trading_ata,
            instruction0,
        );

        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((
                1,
                ProgramError::Custom(ErrorCode::ConstraintAssociated.into()),
            ));
    }
});
