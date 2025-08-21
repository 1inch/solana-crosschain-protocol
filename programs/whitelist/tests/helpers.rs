use anchor_lang::prelude::AccountMeta;
use anchor_lang::InstructionData;
use common_tests::{
    helpers::*,
    whitelist::{
        get_program_whitelist_spec, get_whitelist_access_address, get_whitelist_state_address,
    },
};
use solana_program::{instruction::Instruction, pubkey::Pubkey};

use solana_sdk::{
    signature::Keypair, signer::Signer, system_program::ID as system_program_id,
    transaction::Transaction,
};

use solana_program_test::{BanksClient, ProgramTest, ProgramTestContext};
use std::time::{SystemTime, UNIX_EPOCH};

use test_context::AsyncTestContext;

pub struct TestState {
    pub context: ProgramTestContext,
    pub client: BanksClient,
    pub authority_kp: Keypair,
    pub whitelisted_kp: Keypair,
    pub someone_kp: Keypair,
}

impl AsyncTestContext for TestState {
    async fn setup() -> TestState {
        let mut program_test: ProgramTest = ProgramTest::default();
        add_program_to_test(&mut program_test, "whitelist", || {
            get_program_whitelist_spec()
        });
        let mut context: ProgramTestContext = program_test.start_with_context().await;
        let client: BanksClient = context.banks_client.clone();
        let timestamp: u32 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs()
            .try_into()
            .unwrap();
        set_time(&mut context, timestamp);
        let payer_kp = context.payer.insecure_clone();
        let authority_kp = Keypair::new();
        transfer_lamports(
            &mut context,
            WALLET_DEFAULT_LAMPORTS,
            &payer_kp,
            &authority_kp.pubkey(),
        )
        .await;
        let whitelisted_kp = Keypair::new();
        transfer_lamports(
            &mut context,
            WALLET_DEFAULT_LAMPORTS,
            &payer_kp,
            &whitelisted_kp.pubkey(),
        )
        .await;
        let someone_kp = Keypair::new();
        transfer_lamports(
            &mut context,
            WALLET_DEFAULT_LAMPORTS,
            &payer_kp,
            &someone_kp.pubkey(),
        )
        .await;
        TestState {
            context,
            client,
            authority_kp,
            whitelisted_kp,
            someone_kp,
        }
    }
}

pub fn init_whitelist_data(test_state: &TestState) -> (Pubkey, Transaction) {
    let (whitelist_state, program_id) = get_whitelist_state_address();

    let instruction_data = InstructionData::data(&whitelist::instruction::Initialize {});

    let instruction: Instruction = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(test_state.authority_kp.pubkey(), true),
            AccountMeta::new(whitelist_state, false),
            AccountMeta::new_readonly(system_program_id, false),
        ],
        data: instruction_data,
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test_state.authority_kp.pubkey()),
        &[&test_state.authority_kp],
        test_state.context.last_blockhash,
    );

    (whitelist_state, transaction)
}

pub async fn init_whitelist(test_state: &TestState) -> Pubkey {
    let (whitelist_state, tx) = init_whitelist_data(test_state);
    test_state
        .client
        .process_transaction(tx)
        .await
        .expect_success();
    whitelist_state
}

pub fn register_deregister_data(
    test_state: &TestState,
    instruction_data: Vec<u8>,
) -> (Pubkey, Transaction) {
    let (whitelist_state, program_id) = get_whitelist_state_address();
    let (whitelist_access, _) =
        get_whitelist_access_address(&whitelist::id(), &test_state.whitelisted_kp.pubkey());

    let instruction: Instruction = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(test_state.authority_kp.pubkey(), true),
            AccountMeta::new_readonly(whitelist_state, false),
            AccountMeta::new(whitelist_access, false),
            AccountMeta::new_readonly(system_program_id, false),
        ],
        data: instruction_data,
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test_state.authority_kp.pubkey()),
        &[&test_state.authority_kp],
        test_state.context.last_blockhash,
    );
    (whitelist_access, transaction)
}

pub async fn register(test_state: &TestState, client_program: Pubkey) -> Pubkey {
    let instruction_data = InstructionData::data(&whitelist::instruction::Register {
        _user: test_state.whitelisted_kp.pubkey(),
        _client: client_program,
    });

    let (whitelist_access, tx) = register_deregister_data(test_state, instruction_data);
    test_state
        .client
        .process_transaction(tx)
        .await
        .expect_success();
    whitelist_access
}

pub async fn deregister(test_state: &TestState, client_program: Pubkey) -> Pubkey {
    let instruction_data = InstructionData::data(&whitelist::instruction::Deregister {
        _user: test_state.whitelisted_kp.pubkey(),
        _client: client_program,
    });

    let (whitelist_access, tx) = register_deregister_data(test_state, instruction_data);
    test_state
        .client
        .process_transaction(tx)
        .await
        .expect_success();
    whitelist_access
}

pub fn set_authority_data(test_state: &TestState) -> (Pubkey, Transaction) {
    let (whitelist_state, program_id) = get_whitelist_state_address();
    let instruction_data = InstructionData::data(&whitelist::instruction::SetAuthority {
        new_authority: test_state.someone_kp.pubkey(),
    });

    let instruction: Instruction = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(test_state.authority_kp.pubkey(), true),
            AccountMeta::new(whitelist_state, false),
        ],
        data: instruction_data,
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test_state.authority_kp.pubkey()),
        &[&test_state.authority_kp],
        test_state.context.last_blockhash,
    );
    (whitelist_state, transaction)
}

pub async fn set_authority(test_state: &TestState) -> Pubkey {
    let (whitelist_state, tx) = set_authority_data(test_state);
    test_state
        .client
        .process_transaction(tx)
        .await
        .expect_success();
    whitelist_state
}
