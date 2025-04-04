use anchor_lang::prelude::AccountInfo;
use common_tests::{helpers::*, wrap_entry};
use solana_program::{
    keccak::{hash, Hash},
    pubkey::Pubkey,
};
use solana_program_runtime::invoke_context::BuiltinFunctionWithContext;
use solana_program_test::processor;
use solana_program_test::{BanksClient, ProgramTest, ProgramTestContext};
use solana_sdk::signature::Signer;
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
