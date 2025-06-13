use crate::{dst_program::DstProgram, helpers::*, src_program::SrcProgram};
use anchor_lang::prelude::{AccountInfo, AccountMeta};
use anchor_lang::InstructionData;
use solana_program::{instruction::Instruction, pubkey::Pubkey};
use solana_program_test::processor;
use solana_sdk::signer::keypair::Keypair;

use solana_sdk::{
    signer::Signer, system_program::ID as system_program_id, transaction::Transaction,
};

use solana_program_runtime::invoke_context::BuiltinFunctionWithContext;

use crate::helpers::{EscrowVariant, Expectation, TestStateBase, TokenVariant};
use crate::wrap_entry;

pub fn get_program_mock_spec() -> (Pubkey, Option<BuiltinFunctionWithContext>) {
    (mock::id(), wrap_entry!(mock::entry))
}

pub fn get_mock_create_tx<T, S>(test_state: &TestStateBase<S, T>) -> Transaction {
    let instruction_data = InstructionData::data(&mock::instruction::CreateEscrow { amount: 0 });

    let program_id = mock::id();

    let (escrow_pda, _) = Pubkey::find_program_address(&[b"order"], &program_id);

    let instruction: Instruction = Instruction {
        program_id: mock::id(),
        accounts: vec![
            AccountMeta::new(test_state.token, false),
            AccountMeta::new(test_state.creator_wallet.keypair.pubkey(), false),
            AccountMeta::new(test_state.context.payer.pubkey(), false),
            AccountMeta::new(escrow_pda, false),
            AccountMeta::new_readonly(system_program_id, false),
        ],
        data: instruction_data,
    };

    Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test_state.context.payer.pubkey()),
        &[&test_state.context.payer],
        test_state.context.last_blockhash,
    )
}
