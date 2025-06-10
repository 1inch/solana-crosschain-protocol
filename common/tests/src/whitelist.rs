use anchor_lang::prelude::{AccountInfo, AccountMeta};
use anchor_lang::InstructionData;
use solana_program::{instruction::Instruction, pubkey::Pubkey};
use solana_program_test::processor;

use solana_sdk::{
    signer::Signer, system_program::ID as system_program_id, transaction::Transaction,
};

use solana_program_runtime::invoke_context::BuiltinFunctionWithContext;

use crate::helpers::{EscrowVariant, Expectation, TestStateBase, TokenVariant};
use crate::wrap_entry;

pub fn get_program_whitelist_spec() -> (Pubkey, Option<BuiltinFunctionWithContext>) {
    (whitelist::id(), wrap_entry!(whitelist::entry))
}

pub fn get_whitelist_state_address() -> (Pubkey, Pubkey) {
    let program_id = whitelist::id();
    let (whitelist_state, _) = Pubkey::find_program_address(&[b"whitelist_state"], &program_id);
    (whitelist_state, program_id)
}

pub fn get_whitelist_access_address(user: &Pubkey) -> (Pubkey, u8) {
    let program_id = whitelist::id();
    let (whitelist_access, bump) =
        Pubkey::find_program_address(&[b"resolver_access", user.as_ref()], &program_id);
    (whitelist_access, bump)
}

pub fn init_whitelist_data<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &TestStateBase<T, S>,
) -> (Pubkey, Transaction) {
    let (whitelist_state, program_id) = get_whitelist_state_address();

    let instruction_data = InstructionData::data(&whitelist::instruction::Initialize {});

    let instruction: Instruction = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(test_state.authority_whitelist_kp.pubkey(), true),
            AccountMeta::new(whitelist_state, false),
            AccountMeta::new_readonly(system_program_id, false),
        ],
        data: instruction_data,
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test_state.authority_whitelist_kp.pubkey()),
        &[&test_state.authority_whitelist_kp],
        test_state.context.last_blockhash,
    );

    (whitelist_state, transaction)
}

pub async fn init_whitelist<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &TestStateBase<T, S>,
) -> Pubkey {
    let (whitelist_state, tx) = init_whitelist_data(test_state);
    test_state
        .client
        .process_transaction(tx)
        .await
        .expect_success();
    whitelist_state
}

pub fn register_deregister_data<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &TestStateBase<T, S>,
    whitelisted_account: Pubkey,
    instruction_data: Vec<u8>,
) -> (Pubkey, Transaction) {
    let (whitelist_state, program_id) = get_whitelist_state_address();
    let (whitelist_access, _) = get_whitelist_access_address(&whitelisted_account);

    let instruction: Instruction = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(test_state.authority_whitelist_kp.pubkey(), true),
            AccountMeta::new_readonly(whitelist_state, false),
            AccountMeta::new(whitelist_access, false),
            AccountMeta::new_readonly(system_program_id, false),
        ],
        data: instruction_data,
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test_state.authority_whitelist_kp.pubkey()),
        &[&test_state.authority_whitelist_kp],
        test_state.context.last_blockhash,
    );
    (whitelist_access, transaction)
}

pub async fn register<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &TestStateBase<T, S>,
    whitelisted_account: Pubkey,
) -> Pubkey {
    let instruction_data = InstructionData::data(&whitelist::instruction::Register {
        _user: whitelisted_account,
    });

    let (whitelist_access, tx) =
        register_deregister_data(test_state, whitelisted_account, instruction_data);
    test_state
        .client
        .process_transaction(tx)
        .await
        .expect_success();
    whitelist_access
}

pub async fn deregister<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &TestStateBase<T, S>,
    whitelisted_account: Pubkey,
) -> Pubkey {
    let instruction_data = InstructionData::data(&whitelist::instruction::Deregister {
        _user: whitelisted_account,
    });

    let (whitelist_access, tx) =
        register_deregister_data(test_state, whitelisted_account, instruction_data);
    test_state
        .client
        .process_transaction(tx)
        .await
        .expect_success();
    whitelist_access
}

pub async fn prepare_resolvers<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &TestStateBase<T, S>,
    resolvers: &[Pubkey],
) {
    init_whitelist(test_state).await;

    for resolver in resolvers {
        register(test_state, *resolver).await;
    }
}
