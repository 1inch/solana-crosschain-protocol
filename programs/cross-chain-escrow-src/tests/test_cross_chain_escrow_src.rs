use anchor_lang::prelude::ErrorCode;
use anchor_spl::token::spl_token::state::Account as SplTokenAccount;
use common::error::EscrowError;
use common_tests::helpers::*;
use common_tests::src_program::SrcProgram;
use common_tests::tests as common_escrow_tests;
use solana_program::{program_error::ProgramError, program_pack::Pack};
use solana_program_test::tokio;
use solana_sdk::{signature::Signer, signer::keypair::Keypair, transaction::Transaction};

use test_context::test_context;

type TestState = TestStateBase<SrcProgram>;

mod test_escrow_creation {
    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_with_zero_amount(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation_fail_with_zero_amount(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_with_zero_safety_deposit(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation_fail_with_zero_safety_deposit(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_with_insufficient_funds(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation_fail_with_insufficient_funds(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_with_insufficient_tokens(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation_fail_with_insufficient_tokens(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_with_existing_order_hash(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation_fail_with_existing_order_hash(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_if_finality_duration_overflows(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation_fail_if_finality_duration_overflows(test_state)
            .await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_if_withdrawal_duration_overflows(
        test_state: &mut TestState,
    ) {
        common_escrow_tests::test_escrow_creation_fail_if_withdrawal_duration_overflows(test_state)
            .await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_if_public_withdrawal_duration_overflows(
        test_state: &mut TestState,
    ) {
        common_escrow_tests::test_escrow_creation_fail_if_public_withdrawal_duration_overflows(
            test_state,
        )
        .await
    }

    #[test_context(TestState)]
    #[tokio::test]
    pub async fn test_escrow_creation_fail_if_cancellation_duration_overflows(
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

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_fail_with_invalid_rescue_start(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation_fail_with_invalid_rescue_start(test_state).await
    }
}

mod test_escrow_withdraw {
    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    pub async fn test_withdraw(test_state: &mut TestStateBase<SrcProgram>) {
        common_escrow_tests::test_withdraw(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    pub async fn test_withdraw_does_not_work_with_wrong_secret(
        test_state: &mut TestStateBase<SrcProgram>,
    ) {
        common_escrow_tests::test_withdraw_does_not_work_with_wrong_secret(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    pub async fn test_withdraw_does_not_work_with_non_recipient(
        test_state: &mut TestStateBase<SrcProgram>,
    ) {
        common_escrow_tests::test_withdraw_does_not_work_with_non_recipient(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw_does_not_work_with_wrong_recipient_ata(test_state: &mut TestState) {
        common_escrow_tests::test_withdraw_does_not_work_with_wrong_recipient_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw_does_not_work_with_wrong_escrow_ata(test_state: &mut TestState) {
        common_escrow_tests::test_withdraw_does_not_work_with_wrong_escrow_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw_does_not_work_before_withdrawal_start(test_state: &mut TestState) {
        common_escrow_tests::test_withdraw_does_not_work_before_withdrawal_start(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_withdraw_does_not_work_after_cancellation_start(test_state: &mut TestState) {
        common_escrow_tests::test_withdraw_does_not_work_after_cancellation_start(test_state).await
    }
}

mod test_escrow_public_withdraw {
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
    async fn test_public_withdraw_fails_with_wrong_recipient_ata(test_state: &mut TestState) {
        common_escrow_tests::test_public_withdraw_fails_with_wrong_recipient_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_fails_with_wrong_escrow_ata(test_state: &mut TestState) {
        common_escrow_tests::test_public_withdraw_fails_with_wrong_escrow_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_fails_before_start_of_public_withdraw(
        test_state: &mut TestState,
    ) {
        common_escrow_tests::test_public_withdraw_fails_before_start_of_public_withdraw(test_state)
            .await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_withdraw_fails_after_cancellation_start(test_state: &mut TestState) {
        common_escrow_tests::test_public_withdraw_fails_after_cancellation_start(test_state).await
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
    async fn test_cannot_cancel_with_wrong_escrow_ata(test_state: &mut TestState) {
        common_escrow_tests::test_cannot_cancel_with_wrong_escrow_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cannot_cancel_before_cancellation_start(test_state: &mut TestState) {
        common_escrow_tests::test_cannot_cancel_before_cancellation_start(test_state).await
    }
}

mod test_escrow_public_cancel {
    use super::*;
    use local_helpers::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_public_cancel_by_creator(test_state: &mut TestState) {
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

        let rent_lamports = SrcProgram::get_cached_rent(test_state).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, SplTokenAccount::LEN).await;

        assert_eq!(
            rent_lamports,
            test_state.client.get_balance(escrow).await.unwrap()
        );

        assert_eq!(
            get_token_balance(&mut test_state.context, &escrow_ata).await,
            test_state.test_arguments.escrow_amount
        );

        test_state
            .expect_balance_change(
                transaction,
                &[
                    native_change(
                        test_state.creator_wallet.keypair.pubkey(),
                        rent_lamports + token_account_rent,
                    ),
                    token_change(
                        test_state.creator_wallet.token_account,
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

        let rent_lamports = SrcProgram::get_cached_rent(test_state).await;

        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, SplTokenAccount::LEN).await;

        assert_eq!(
            get_token_balance(&mut test_state.context, &escrow_ata).await,
            test_state.test_arguments.escrow_amount
        );

        assert_eq!(
            rent_lamports,
            test_state.client.get_balance(escrow).await.unwrap()
        );

        test_state
            .expect_balance_change(
                transaction,
                &[
                    token_change(
                        test_state.creator_wallet.token_account,
                        test_state.test_arguments.escrow_amount,
                    ),
                    native_change(canceller.pubkey(), test_state.test_arguments.safety_deposit),
                    native_change(
                        test_state.creator_wallet.keypair.pubkey(),
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
    async fn test_public_cancel_fail_with_wrong_creator_ata(test_state: &mut TestState) {
        let (escrow, escrow_ata) = create_escrow(test_state).await;
        test_state.creator_wallet.token_account = test_state.recipient_wallet.token_account;

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
    async fn test_public_cancel_fail_with_wrong_escrow_ata(test_state: &mut TestState) {
        let (escrow, _) = create_escrow(test_state).await;

        test_state.test_arguments.escrow_amount += 1;
        let (_, escrow_ata_2) = create_escrow(test_state).await;

        let canceller = Keypair::new();
        transfer_lamports(
            &mut test_state.context,
            WALLET_DEFAULT_LAMPORTS,
            &test_state.payer_kp,
            &canceller.pubkey(),
        )
        .await;
        let transaction = create_public_cancel_tx(test_state, &escrow, &escrow_ata_2, &canceller);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp
                + DEFAULT_PERIOD_DURATION * PeriodType::PublicCancellation as u32,
        );

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
    async fn test_cannot_public_cancel_before_public_cancellation_start(
        test_state: &mut TestState,
    ) {
        let (escrow, escrow_ata) = create_escrow(test_state).await;
        let transaction =
            create_public_cancel_tx(test_state, &escrow, &escrow_ata, &test_state.payer_kp);

        set_time(
            &mut test_state.context,
            test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
        );
        test_state
            .client
            .process_transaction(transaction)
            .await
            .expect_error((0, ProgramError::Custom(EscrowError::InvalidTime.into())))
    }
}

mod test_escrow_rescue_funds {
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
        common_escrow_tests::test_cannot_rescue_funds_before_rescue_delay_pass(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cannot_rescue_funds_by_non_recipient(test_state: &mut TestState) {
        common_escrow_tests::test_cannot_rescue_funds_by_non_recipient(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cannot_rescue_funds_with_wrong_recipient_ata(test_state: &mut TestState) {
        common_escrow_tests::test_cannot_rescue_funds_with_wrong_recipient_ata(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_cannot_rescue_funds_with_wrong_escrow_ata(test_state: &mut TestState) {
        common_escrow_tests::test_cannot_rescue_funds_with_wrong_escrow_ata(test_state).await
    }
}

mod local_helpers {
    use super::*;

    use anchor_lang::InstructionData;
    use anchor_spl::token::spl_token::ID as spl_program_id;
    use solana_program::instruction::{AccountMeta, Instruction};
    use solana_program::pubkey::Pubkey;
    use solana_program::system_program::ID as system_program_id;
    use solana_sdk::signature::Signer;

    pub fn create_public_cancel_tx(
        test_state: &TestState,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
        canceller: &Keypair,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_src::instruction::PublicCancel {});

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_src::id(),
            accounts: vec![
                AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), false),
                AccountMeta::new_readonly(test_state.token, false),
                AccountMeta::new(canceller.pubkey(), true),
                AccountMeta::new(*escrow, false),
                AccountMeta::new(*escrow_ata, false),
                AccountMeta::new(test_state.creator_wallet.token_account, false),
                AccountMeta::new_readonly(spl_program_id, false),
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
}
