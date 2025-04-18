use anchor_lang::prelude::AccountInfo;
use anchor_spl::token::spl_token::state::Account as SplTokenAccount;
use common::error::EscrowError;
use common_tests::helpers::*;
use common_tests::tests as common_escrow_tests;

use anchor_lang::{InstructionData, Space};
use anchor_spl::{
    associated_token::ID as spl_associated_token_id, token::spl_token::ID as spl_program_id,
};
use common_tests::wrap_entry;
use solana_program::{
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    system_program::ID as system_program_id,
    sysvar::rent::ID as rent_id,
};
use solana_program_runtime::invoke_context::BuiltinFunctionWithContext;
use solana_program_test::{processor, tokio};
use solana_sdk::{
    signature::Signer, signer::keypair::Keypair, sysvar::clock::Clock, transaction::Transaction,
};
use test_context::test_context;

type TestState = TestStateBase<DstProgram>;

struct DstProgram;

impl EscrowVariant for DstProgram {
    fn get_program_spec() -> (Pubkey, Option<BuiltinFunctionWithContext>) {
        (
            cross_chain_escrow_dst::id(),
            wrap_entry!(cross_chain_escrow_dst::entry),
        )
    }

    fn get_public_withdraw_tx(
        test_state: &TestState,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
        withdrawer: &Keypair,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_dst::instruction::PublicWithdraw {
                secret: test_state.secret,
            });

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_dst::id(),
            accounts: vec![
                AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), false),
                AccountMeta::new_readonly(test_state.recipient_wallet.keypair.pubkey(), false),
                AccountMeta::new(withdrawer.pubkey(), true),
                AccountMeta::new_readonly(test_state.token, false),
                AccountMeta::new(*escrow, false),
                AccountMeta::new(*escrow_ata, false),
                AccountMeta::new(test_state.recipient_wallet.token_account, false),
                AccountMeta::new_readonly(spl_program_id, false),
                AccountMeta::new_readonly(system_program_id, false),
            ],
            data: instruction_data,
        };

        Transaction::new_signed_with_payer(
            &[instruction],
            Some(&test_state.payer_kp.pubkey()), // so that withdrawer does not incurr transaction
            // charges and mess up computation of withdrawer's
            // balance expectation.
            &[withdrawer, &test_state.payer_kp],
            test_state.context.last_blockhash,
        )
    }

    fn get_withdraw_tx(
        test_state: &TestState,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_dst::instruction::Withdraw {
                secret: test_state.secret,
            });

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_dst::id(),
            accounts: vec![
                AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), true),
                AccountMeta::new_readonly(test_state.recipient_wallet.keypair.pubkey(), false),
                AccountMeta::new_readonly(test_state.token, false),
                AccountMeta::new(*escrow, false),
                AccountMeta::new(*escrow_ata, false),
                AccountMeta::new(test_state.recipient_wallet.token_account, false),
                AccountMeta::new_readonly(spl_program_id, false),
                AccountMeta::new_readonly(system_program_id, false),
            ],
            data: instruction_data,
        };

        Transaction::new_signed_with_payer(
            &[instruction],
            Some(&test_state.payer_kp.pubkey()),
            &[
                &test_state.context.payer,
                &test_state.creator_wallet.keypair,
            ],
            test_state.context.last_blockhash,
        )
    }

    fn get_cancel_tx(
        test_state: &TestStateBase<DstProgram>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_dst::instruction::Cancel {});

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_dst::id(),
            accounts: vec![
                AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), true),
                AccountMeta::new_readonly(test_state.token, false),
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
            &[
                &test_state.context.payer,
                &test_state.creator_wallet.keypair,
            ],
            test_state.context.last_blockhash,
        )
    }

    fn get_create_tx(
        test_state: &TestStateBase<DstProgram>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_dst::instruction::Create {
                amount: test_state.test_arguments.escrow_amount,
                order_hash: test_state.order_hash.to_bytes(),
                hashlock: test_state.hashlock.to_bytes(),
                recipient: test_state.recipient_wallet.keypair.pubkey(),
                safety_deposit: test_state.test_arguments.safety_deposit,
                finality_duration: test_state.test_arguments.finality_duration,
                public_withdrawal_duration: test_state.test_arguments.public_withdrawal_duration,
                withdrawal_duration: test_state.test_arguments.withdrawal_duration,
                src_cancellation_timestamp: test_state.test_arguments.src_cancellation_timestamp,
                rescue_start: test_state.test_arguments.rescue_start,
            });

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_dst::id(),
            accounts: vec![
                AccountMeta::new(test_state.payer_kp.pubkey(), true),
                AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), true),
                AccountMeta::new_readonly(test_state.token, false),
                AccountMeta::new(test_state.creator_wallet.token_account, false),
                AccountMeta::new(*escrow, false),
                AccountMeta::new(*escrow_ata, false),
                AccountMeta::new_readonly(spl_associated_token_id, false),
                AccountMeta::new_readonly(spl_program_id, false),
                AccountMeta::new_readonly(rent_id, false),
                AccountMeta::new_readonly(system_program_id, false),
            ],
            data: instruction_data,
        };
        Transaction::new_signed_with_payer(
            &[instruction],
            Some(&test_state.payer_kp.pubkey()),
            &[
                &test_state.context.payer,
                &test_state.creator_wallet.keypair,
            ],
            test_state.context.last_blockhash,
        )
    }

    fn get_rescue_funds_tx(
        test_state: &TestState,
        escrow: &Pubkey,
        token_to_rescue: &Pubkey,
        escrow_ata: &Pubkey,
        recipient_ata: &Pubkey,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_dst::instruction::RescueFunds {
                hashlock: test_state.hashlock.to_bytes(),
                order_hash: test_state.order_hash.to_bytes(),
                escrow_creator: test_state.creator_wallet.keypair.pubkey(),
                escrow_mint: test_state.token,
                escrow_amount: test_state.test_arguments.escrow_amount,
                safety_deposit: test_state.test_arguments.safety_deposit,
                rescue_start: test_state.test_arguments.rescue_start,
                rescue_amount: test_state.test_arguments.rescue_amount,
            });

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_dst::id(),
            accounts: vec![
                AccountMeta::new(test_state.recipient_wallet.keypair.pubkey(), true),
                AccountMeta::new_readonly(*token_to_rescue, false),
                AccountMeta::new(*escrow, false),
                AccountMeta::new(*escrow_ata, false),
                AccountMeta::new(*recipient_ata, false),
                AccountMeta::new_readonly(spl_program_id, false),
                AccountMeta::new_readonly(system_program_id, false),
            ],
            data: instruction_data,
        };

        Transaction::new_signed_with_payer(
            &[instruction],
            Some(&test_state.payer_kp.pubkey()),
            &[
                &test_state.context.payer,
                &test_state.recipient_wallet.keypair,
            ],
            test_state.context.last_blockhash,
        )
    }

    fn get_escrow_data_len() -> usize {
        cross_chain_escrow_dst::constants::DISCRIMINATOR
            + cross_chain_escrow_dst::EscrowDst::INIT_SPACE
    }
}

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
    async fn test_escrow_creation_fail_when_cancellation_start_gt_src_cancellation_timestamp(
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
    pub async fn test_withdraw(test_state: &mut TestStateBase<DstProgram>) {
        common_escrow_tests::test_withdraw(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    pub async fn test_withdraw_does_not_work_with_wrong_secret(
        test_state: &mut TestStateBase<DstProgram>,
    ) {
        common_escrow_tests::test_withdraw_does_not_work_with_wrong_secret(test_state).await
    }

    #[test_context(TestState)]
    #[tokio::test]
    pub async fn test_withdraw_does_not_work_with_non_recipient(
        test_state: &mut TestStateBase<DstProgram>,
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
    async fn test_public_withdraw_tokens_by_creator(test_state: &mut TestState) {
        let (escrow, escrow_ata) = create_escrow(test_state).await;

        let transaction = DstProgram::get_public_withdraw_tx(
            test_state,
            &escrow,
            &escrow_ata,
            &test_state.creator_wallet.keypair,
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
        let escrow_data_len = DstProgram::get_escrow_data_len();
        let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;
        let token_account_rent =
            get_min_rent_for_size(&mut test_state.client, SplTokenAccount::LEN).await;
        assert_eq!(
            rent_lamports,
            test_state.client.get_balance(escrow).await.unwrap()
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
                        test_state.recipient_wallet.token_account,
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

mod test_escrow_creation_cost {
    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_tx_cost(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation_tx_cost(test_state).await
    }
}
