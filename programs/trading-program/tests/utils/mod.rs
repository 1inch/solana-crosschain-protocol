use anchor_lang::{prelude::AccountInfo, InstructionData};
use anchor_spl::{
    associated_token::ID as spl_associated_token_id, token::spl_token::ID as spl_program_id,
};
use common_tests::src_program::SrcProgram;
use common_tests::{helpers::*, wrap_entry};
use solana_program::{
    instruction::{AccountMeta, Instruction},
    keccak::{hash, Hash},
    pubkey::Pubkey,
    system_program::ID as system_program_id,
    sysvar::rent::ID as rent_id,
};
use solana_program_runtime::invoke_context::BuiltinFunctionWithContext;
use solana_program_test::processor;
use solana_program_test::{BanksClient, ProgramTest, ProgramTestContext};
use solana_sdk::{signature::Signer, transaction::Transaction};
use std::marker::PhantomData;
use std::time::{SystemTime, UNIX_EPOCH};
use test_context::AsyncTestContext;

pub struct TestStateTrading<T: ?Sized> {
    pub base: TestStateBase<T>,
}

fn get_trading_program_spec() -> (Pubkey, Option<BuiltinFunctionWithContext>) {
    (trading_program::id(), wrap_entry!(trading_program::entry))
}

impl<T> AsyncTestContext for TestStateTrading<T>
where
    T: EscrowVariant,
{
    async fn setup() -> TestStateTrading<T> {
        let mut program_test: ProgramTest = ProgramTest::default();
        add_program_to_test(&mut program_test, "escrow_contract", T::get_program_spec);
        add_program_to_test(
            &mut program_test,
            "trading_program",
            get_trading_program_spec,
        );
        let mut context: ProgramTestContext = program_test.start_with_context().await;

        let client: BanksClient = context.banks_client.clone();
        let timestamp: u32 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs()
            .try_into()
            .unwrap();

        set_time(&mut context, timestamp);
        let token = deploy_spl_token(&mut context, 8).await.pubkey();
        let secret = hash(b"default_secret").to_bytes();
        let payer_kp = context.payer.insecure_clone();
        let creator_wallet = create_wallet(
            &mut context,
            &token,
            WALLET_DEFAULT_LAMPORTS,
            WALLET_DEFAULT_TOKENS,
        )
        .await;
        let recipient_wallet = create_wallet(
            &mut context,
            &token,
            WALLET_DEFAULT_LAMPORTS,
            WALLET_DEFAULT_TOKENS,
        )
        .await;
        TestStateTrading {
            base: TestStateBase {
                context,
                client,
                secret,
                order_hash: Hash::new_unique(),
                hashlock: hash(secret.as_ref()),
                token,
                payer_kp: payer_kp.insecure_clone(),
                creator_wallet,
                recipient_wallet,
                init_timestamp: timestamp,
                test_arguments: get_default_testargs(timestamp),
                pd: PhantomData,
            },
        }
    }
}

pub fn create_escrow_via_trading_program(
    test_state: &TestStateBase<SrcProgram>,
) -> (Pubkey, Pubkey, Transaction) {
    let (escrow_pda, escrow_ata) = get_escrow_addresses(test_state);
    let (src_program_id, _) = SrcProgram::get_program_spec();

    let instruction: Instruction = Instruction {
        program_id: trading_program::id(),
        accounts: vec![
            AccountMeta::new_readonly(test_state.recipient_wallet.keypair.pubkey(), true), // taker
            AccountMeta::new_readonly(test_state.creator_wallet.keypair.pubkey(), false), // maker
            AccountMeta::new_readonly(test_state.trading_account, false), // trading_account
            AccountMeta::new_readonly(test_state.trading_account_ata, false), // trading_account_tokens
            AccountMeta::new(escrow_pda, false), // escrow
            AccountMeta::new_readonly(test_state.token, false), // token
            AccountMeta::new(escrow_ata, false), // escrow_tokens
            // ix_sysvar
            AccountMeta::new_readonly(spl_associated_token_id, false),
            AccountMeta::new_readonly(spl_program_id, false),
            AccountMeta::new_readonly(rent_id, false),
            AccountMeta::new_readonly(system_program_id, false),
            AccountMeta::new_readonly(src_program_id, false),
        ],
        data: InstructionData::data(&trading_program::instruction::InitEscrowSrc {
            src_cancellation_timestamp: test_state.test_arguments.src_cancellation_timestamp,
            rescue_start: test_state.test_arguments.rescue_start,
        }),
    };

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test_state.recipient_wallet.keypair.pubkey()),
        &[
            &test_state.recipient_wallet.keypair,
        ],
        test_state.context.last_blockhash,
    );

    (escrow_pda, escrow_ata, transaction)
}
