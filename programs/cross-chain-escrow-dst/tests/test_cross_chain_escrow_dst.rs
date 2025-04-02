use anchor_lang::prelude::AccountInfo;
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
    pubkey::Pubkey,
    system_program::ID as system_program_id,
    sysvar::rent::ID as rent_id,
};
use solana_program_runtime::invoke_context::BuiltinFunctionWithContext;
use solana_program_test::{processor, tokio};
use solana_sdk::{signature::Signer, sysvar::clock::Clock, transaction::Transaction};
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

    fn get_public_withdraw_ix(
        test_state: &TestState,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Instruction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_dst::instruction::PublicWithdraw {
                secret: test_state.secret,
            });

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_dst::id(),
            accounts: vec![
                AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), false),
                AccountMeta::new_readonly(test_state.recipient_wallet.keypair.pubkey(), false),
                AccountMeta::new_readonly(test_state.context.payer.pubkey(), false),
                AccountMeta::new_readonly(test_state.token, false),
                AccountMeta::new(*escrow, false),
                AccountMeta::new(*escrow_ata, false),
                AccountMeta::new(test_state.recipient_wallet.token_account, false),
                AccountMeta::new_readonly(spl_program_id, false),
                AccountMeta::new_readonly(system_program_id, false),
            ],
            data: instruction_data,
        };

        instruction
    }

    fn withdraw_ix_to_signed_tx(ix: Instruction, test_state: &TestState) -> Transaction {
        Transaction::new_signed_with_payer(
            &[ix],
            Some(&test_state.payer_kp.pubkey()),
            &[
                &test_state.context.payer,
                &test_state.creator_wallet.keypair,
            ],
            test_state.context.last_blockhash,
        )
    }
    fn get_withdraw_ix(
        test_state: &TestState,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Instruction {
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

        instruction
    }

    fn get_cancel_ix(
        test_state: &TestStateBase<DstProgram>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Instruction {
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

        instruction
    }

    fn get_create_ix(
        test_state: &TestStateBase<DstProgram>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Instruction {
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
        instruction
    }

    fn get_rescue_funds_ix(
        test_state: &TestState,
        escrow: &Pubkey,
        token_to_rescue: &Pubkey,
        escrow_ata: &Pubkey,
        recipient_ata: &Pubkey,
    ) -> Instruction {
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

        instruction
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
    async fn test_escrow_creation_fail_with_insufficient_safety_deposit(
        test_state: &mut TestState,
    ) {
        common_escrow_tests::test_escrow_creation_fail_with_insufficient_safety_deposit(test_state)
            .await
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
        let (_, _, create_ix) = create_escrow_data(test_state);

        let transaction = Transaction::new_signed_with_payer(
            &[create_ix],
            Some(&test_state.payer_kp.pubkey()),
            &[
                &test_state.context.payer,
                &test_state.creator_wallet.keypair,
            ],
            test_state.context.last_blockhash,
        );

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
