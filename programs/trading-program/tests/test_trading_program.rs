use anchor_lang::error::ErrorCode;
use common_tests::{helpers::*, src_program::SrcProgram};
use std::marker::PhantomData;

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
    use solana_sdk::signature::Keypair;

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

    pub fn get_token_account_size<S: TokenVariant>(
        _: PhantomData<TestStateTrading<S>>,
    ) -> usize {
        S::get_token_account_size()
    }


    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_withdrawal_via_trading_program_for_resolver(
        test_state_trading: &mut TestState,
    ) {
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
        let escrow_rent_lamports =
            get_min_rent_for_size(&mut test_state.client, <SrcProgram as EscrowVariant<Token2020>>::get_escrow_data_len()).await;
        assert_eq!(
            escrow_rent_lamports,
            test_state.client.get_balance(escrow).await.unwrap()
        );

        // Get the token account rent
        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, get_token_account_size(PhantomData::<TestState>)).await;

        // Create the transaction to withdraw from the escrow
        let transaction = build_withdraw_tx_src(
            test_state,
            &escrow,
            &escrow_ata,
            Some(&test_state.recipient_wallet.keypair.pubkey()),
        );

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
        );

        test_state
            .expect_balance_change(
                transaction,
                &[
                    // Fails due to recipient not getting his account rent back,
                    // trading_pda (aka creator defined) receives it instead
                    native_change(
                        test_state.recipient_wallet.keypair.pubkey(),
                        escrow_rent_lamports + token_account_rent,
                    ),
                    token_change(
                        test_state.recipient_wallet.token_account,
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
    async fn test_escrow_public_withdrawal_via_trading_program_for_resolver(
        test_state_trading: &mut TestState,
    ) {
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
        let escrow_rent_lamports =
            get_min_rent_for_size(&mut test_state.client, <SrcProgram as EscrowVariant<Token2020>>::get_escrow_data_len()).await;
        assert_eq!(
            escrow_rent_lamports,
            test_state.client.get_balance(escrow).await.unwrap()
        );

        // Get the token account rent
        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, get_token_account_size(PhantomData::<TestState>)).await;
        let withdrawer = test_state.recipient_wallet.keypair.insecure_clone();

        // Create the transaction to withdraw from the escrow
        let transaction = build_public_withdraw_tx_src(
            test_state,
            &escrow,
            &escrow_ata,
            &withdrawer,
            Some(&test_state.recipient_wallet.keypair.pubkey()),
        );

        // Waiting for the public withdrawal period
        set_time(
            &mut test_state.context,
            test_state.init_timestamp
                + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
        );

        test_state
            .expect_balance_change(
                transaction,
                &[
                    // Fails due to recipient not getting his account rent back + safety deposit,
                    // trading_pda (aka creator defined) receives it instead
                    native_change(
                        withdrawer.pubkey(),
                        escrow_rent_lamports + token_account_rent,
                    ),
                    token_change(
                        test_state.recipient_wallet.token_account,
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
    async fn test_escrow_public_withdrawal_via_trading_program_for_any_account(
        test_state_trading: &mut TestState,
    ) {
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
        let escrow_rent_lamports =
            get_min_rent_for_size(&mut test_state.client, <SrcProgram as EscrowVariant<Token2020>>::get_escrow_data_len()).await;
        assert_eq!(
            escrow_rent_lamports,
            test_state.client.get_balance(escrow).await.unwrap()
        );

        // Get the token account rent
        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, get_token_account_size(PhantomData::<TestState>)).await;

        let withdrawer = Keypair::new();
        transfer_lamports(
            &mut test_state.context,
            WALLET_DEFAULT_LAMPORTS,
            &test_state.payer_kp,
            &withdrawer.pubkey(),
        )
        .await;

        // Create the transaction to withdraw from the escrow
        let transaction = build_public_withdraw_tx_src(
            test_state,
            &escrow,
            &escrow_ata,
            &withdrawer,
            Some(&test_state.recipient_wallet.keypair.pubkey()),
        );

        // Waiting for the public withdrawal period
        set_time(
            &mut test_state.context,
            test_state.init_timestamp
                + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
        );

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(
                        withdrawer.pubkey(),
                        test_state.test_arguments.safety_deposit,
                    ),
                    // Fails due to recipient not getting his account rent back - safety deposit,
                    // trading_pda (aka creator defined) receives it instead
                    native_change(
                        test_state.recipient_wallet.keypair.pubkey(),
                        escrow_rent_lamports + token_account_rent
                            - test_state.test_arguments.safety_deposit,
                    ),
                    token_change(
                        test_state.recipient_wallet.token_account,
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
});
