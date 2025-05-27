use anchor_lang::{prelude::AccountInfo, AnchorSerialize, InstructionData};
use anchor_spl::{
    associated_token::{get_associated_token_address, ID as spl_associated_token_id},
    token::spl_token::ID as spl_program_id,
};
use common_tests::src_program::SrcProgram;
use common_tests::{helpers::*, wrap_entry};
use ed25519_dalek::Keypair as DalekKeypair;
use solana_program::{
    instruction::{AccountMeta, Instruction},
    keccak::{hash, Hash},
    pubkey::Pubkey,
    system_program::ID as system_program_id,
    sysvar::{instructions::ID as ix_sysvar_id, rent::ID as rent_id},
};
use solana_program_runtime::invoke_context::BuiltinFunctionWithContext;
use solana_program_test::{processor, BanksClient, ProgramTest, ProgramTestContext};
use solana_sdk::{
    ed25519_instruction::new_ed25519_instruction,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

use std::marker::PhantomData;
use std::time::{SystemTime, UNIX_EPOCH};
use test_context::AsyncTestContext;
use trading_program::{constants::SEED_PREFIX, utils::Order};

pub struct TestStateTrading {
    pub base: TestStateBase<SrcProgram>,
}

fn get_trading_program_spec() -> (Pubkey, Option<BuiltinFunctionWithContext>) {
    (trading_program::id(), wrap_entry!(trading_program::entry))
}

impl AsyncTestContext for TestStateTrading {
    async fn setup() -> TestStateTrading {
        let mut program_test: ProgramTest = ProgramTest::default();
        add_program_to_test(
            &mut program_test,
            "escrow_contract",
            SrcProgram::get_program_spec,
        );
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

fn get_trading_addresses(test_state: &TestStateBase<SrcProgram>) -> (Pubkey, Pubkey) {
    let (trading_pda, _) = Pubkey::find_program_address(
        &[
            SEED_PREFIX,
            test_state.creator_wallet.keypair.pubkey().as_ref(),
        ],
        &trading_program::id(),
    );
    let trading_ata = get_associated_token_address(&trading_pda, &test_state.token);

    (trading_pda, trading_ata)
}

pub async fn prepare_trading_account(
    test_state: &mut TestStateBase<SrcProgram>,
) -> (Pubkey, Pubkey, Pubkey, Pubkey) {
    let (trading_pda, _) = get_trading_addresses(test_state);
    let (escrow_pda, escrow_ata) = get_escrow_addresses(test_state, trading_pda);

    let trading_ata =
        initialize_spl_associated_account(&mut test_state.context, &test_state.token, &trading_pda)
            .await;
    mint_spl_tokens(
        &mut test_state.context,
        &test_state.token,
        &trading_ata,
        test_state.test_arguments.escrow_amount,
    )
    .await;
    (escrow_pda, escrow_ata, trading_pda, trading_ata)
}

pub fn create_signinig_default_order_ix(
    test_state: &mut TestStateBase<SrcProgram>,
    signer: Keypair,
) -> Instruction {
    let order = Order {
        order_hash: test_state.order_hash.to_bytes(),
        hashlock: test_state.hashlock.to_bytes(),
        maker: test_state.creator_wallet.keypair.pubkey(),
        token: test_state.token,
        amount: test_state.test_arguments.escrow_amount,
        safety_deposit: test_state.test_arguments.safety_deposit,
        finality_duration: test_state.test_arguments.finality_duration,
        withdrawal_duration: test_state.test_arguments.withdrawal_duration,
        public_withdrawal_duration: test_state.test_arguments.public_withdrawal_duration,
        cancellation_duration: test_state.test_arguments.cancellation_duration,
        rescue_start: test_state.test_arguments.rescue_start,
    };
    let order_bytes = order.try_to_vec().unwrap();

    let dalek_kp = DalekKeypair::from_bytes(&signer.to_bytes()).unwrap();
    new_ed25519_instruction(&dalek_kp, &order_bytes)
}

pub fn init_escrow_src_tx(
    test_state: &mut TestStateBase<SrcProgram>,
    escrow_pda: Pubkey,
    escrow_ata: Pubkey,
    trading_pda: Pubkey,
    trading_ata: Pubkey,
    instruction0: Instruction,
) -> Transaction {
    let instruction1: Instruction = Instruction {
        program_id: trading_program::id(),
        accounts: vec![
            AccountMeta::new(test_state.recipient_wallet.keypair.pubkey(), true), // taker
            AccountMeta::new(trading_pda, false),                                 // trading_account
            AccountMeta::new(trading_ata, false), // trading_account_ata
            AccountMeta::new(escrow_pda, false),  // escrow
            AccountMeta::new_readonly(test_state.token, false), // token
            AccountMeta::new(escrow_ata, false),  // escrow_ata
            AccountMeta::new_readonly(ix_sysvar_id, false),
            AccountMeta::new_readonly(spl_associated_token_id, false),
            AccountMeta::new_readonly(spl_program_id, false),
            AccountMeta::new_readonly(rent_id, false),
            AccountMeta::new_readonly(system_program_id, false),
            AccountMeta::new_readonly(cross_chain_escrow_src::id(), false),
        ],
        data: InstructionData::data(&trading_program::instruction::InitEscrowSrc {}),
    };

    Transaction::new_signed_with_payer(
        &[instruction0, instruction1],
        Some(&test_state.recipient_wallet.keypair.pubkey()),
        &[&test_state.recipient_wallet.keypair],
        test_state.context.last_blockhash,
    )
}

pub async fn create_escrow_via_trading_program(
    test_state: &mut TestStateBase<SrcProgram>,
) -> (Pubkey, Pubkey, Pubkey, Pubkey) {
    let (escrow_pda, escrow_ata, trading_pda, trading_ata) =
        prepare_trading_account(test_state).await;

    let instruction0 = create_signinig_default_order_ix(
        test_state,
        test_state.creator_wallet.keypair.insecure_clone(),
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
        .expect_success();

    (escrow_pda, escrow_ata, trading_pda, trading_ata)
}
