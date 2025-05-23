use crate::helpers::*;
use anchor_lang::prelude::AccountInfo;
use anchor_spl::token::spl_token::native_mint::ID as NATIVE_MINT;
use solana_program_runtime::invoke_context::BuiltinFunctionWithContext;
use solana_sdk::{signature::Signer, signer::keypair::Keypair, transaction::Transaction};

use crate::wrap_entry;
use anchor_lang::InstructionData;
use solana_program_test::processor;

use anchor_lang::Space;
use anchor_spl::{
    associated_token::ID as spl_associated_token_id, token::spl_token::ID as spl_program_id,
};

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program::ID as system_program_id,
    sysvar::rent::ID as rent_id,
};

pub struct SrcProgram;

type TestState = TestStateBase<SrcProgram>;

impl EscrowVariant for SrcProgram {
    fn get_program_spec() -> (Pubkey, Option<BuiltinFunctionWithContext>) {
        (
            cross_chain_escrow_src::id(),
            wrap_entry!(cross_chain_escrow_src::entry),
        )
    }

    fn get_escrow_data_len() -> usize {
        cross_chain_escrow_src::constants::DISCRIMINATOR
            + cross_chain_escrow_src::EscrowSrc::INIT_SPACE
    }

    fn get_create_tx(
        test_state: &TestStateBase<SrcProgram>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_src::instruction::Create {
                amount: test_state.test_arguments.escrow_amount,
                order_hash: test_state.order_hash.to_bytes(),
                hashlock: test_state.hashlock.to_bytes(),
                recipient: test_state.recipient_wallet.keypair.pubkey(),
                safety_deposit: test_state.test_arguments.safety_deposit,
                cancellation_duration: test_state.test_arguments.cancellation_duration,
                finality_duration: test_state.test_arguments.finality_duration,
                public_withdrawal_duration: test_state.test_arguments.public_withdrawal_duration,
                withdrawal_duration: test_state.test_arguments.withdrawal_duration,
                rescue_start: test_state.test_arguments.rescue_start,
            });

        let creator_ata = if test_state.token == NATIVE_MINT {
            cross_chain_escrow_src::id()
        } else {
            test_state.creator_wallet.token_account
        };

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_src::id(),
            accounts: vec![
                AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), true),
                AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), true),
                AccountMeta::new_readonly(test_state.token, false),
                AccountMeta::new(creator_ata, false),
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
            Some(&test_state.creator_wallet.keypair.pubkey()),
            &[&test_state.creator_wallet.keypair],
            test_state.context.last_blockhash,
        )
    }

    fn get_withdraw_tx(
        test_state: &TestState,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction {
        build_withdraw_tx_src(test_state, escrow, escrow_ata, None, None)
    }

    fn get_public_withdraw_tx(
        test_state: &TestState,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
        withdrawer: &Keypair,
    ) -> Transaction {
        build_public_withdraw_tx_src(test_state, escrow, escrow_ata, withdrawer, None, None)
    }

    fn get_cancel_tx(
        test_state: &TestStateBase<SrcProgram>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction {
        build_cancel_tx(test_state, escrow, escrow_ata, None, None, None)
    }

    fn get_rescue_funds_tx(
        test_state: &TestState,
        escrow: &Pubkey,
        token_to_rescue: &Pubkey,
        escrow_ata: &Pubkey,
        recipient_ata: &Pubkey,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_src::instruction::RescueFunds {
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
            program_id: cross_chain_escrow_src::id(),
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
}
