use anchor_lang::prelude::{AccountInfo, AccountMeta};
use anchor_lang::InstructionData;
use common::constants::DISCRIMINATOR;
use common_tests::{helpers::*, wrap_entry};
use solana_program::{instruction::Instruction, pubkey::Pubkey};
use solana_program_test::{processor, tokio};

use solana_sdk::{
    signature::Keypair, signer::Signer, system_program::ID as system_program_id,
    transaction::Transaction,
};

use solana_program_runtime::invoke_context::BuiltinFunctionWithContext;
use solana_program_test::{BanksClient, ProgramTest, ProgramTestContext};
use std::time::{SystemTime, UNIX_EPOCH};

use test_context::AsyncTestContext;

use test_context::test_context;

pub struct TestState {
    pub context: ProgramTestContext,
    pub client: BanksClient,
    pub payer_kp: Keypair,
    pub creator_kp: Keypair,
    pub recipient_kp: Keypair,
}

fn get_program_whitelist_spec() -> (Pubkey, Option<BuiltinFunctionWithContext>) {
    (whitelist::id(), wrap_entry!(whitelist::entry))
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
        let creator_kp = Keypair::new();
        transfer_lamports(
            &mut context,
            WALLET_DEFAULT_LAMPORTS,
            &payer_kp,
            &creator_kp.pubkey(),
        )
        .await;
        let recipient_kp = Keypair::new();
        transfer_lamports(
            &mut context,
            WALLET_DEFAULT_LAMPORTS,
            &payer_kp,
            &recipient_kp.pubkey(),
        )
        .await;
        TestState {
            context,
            client,
            payer_kp,
            creator_kp,
            recipient_kp,
        }
    }
}

pub fn init_whitelist(test_state: &TestState) -> (Pubkey, Transaction) {
    let program_id = whitelist::id();
    let (whitelist_state, _) = Pubkey::find_program_address(&[b"whitelist_state"], &program_id);

    let instruction_data = InstructionData::data(&whitelist::instruction::Initialize {});

    let instruction: Instruction = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(test_state.creator_kp.pubkey(), true),
            AccountMeta::new(whitelist_state, false),
            AccountMeta::new_readonly(system_program_id, false),
        ],
        data: instruction_data,
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test_state.creator_kp.pubkey()),
        &[&test_state.creator_kp],
        test_state.context.last_blockhash,
    );

    (whitelist_state, transaction)
}

mod test_whitelist {
    use anchor_lang::Space;

    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_init_whitelist(test_state: &mut TestState) {
        let (whitelist_state, init_tx) = init_whitelist(test_state);
        test_state
            .client
            .process_transaction(init_tx)
            .await
            .expect_success();

        let whitelist_data_len = DISCRIMINATOR + whitelist::WhitelistState::INIT_SPACE;
        let rent_lamports = get_min_rent_for_size(&mut test_state.client, whitelist_data_len).await;
        assert_eq!(
            rent_lamports,
            test_state
                .client
                .get_balance(whitelist_state)
                .await
                .unwrap()
        );
    }
}
