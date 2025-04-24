use anchor_spl::associated_token::{
    get_associated_token_address,
    spl_associated_token_account::instruction::create_associated_token_account,
};
use anchor_spl::token::spl_token::{
    instruction as spl_instruction,
    state::{Account as SplTokenAccount, Mint},
    ID as spl_program_id,
};
use common::constants::RESCUE_DELAY;
use solana_program::{
    instruction::Instruction,
    keccak::{hash, Hash},
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
};
use solana_program_runtime::invoke_context::BuiltinFunctionWithContext;
use solana_program_test::{BanksClient, BanksClientError, ProgramTest, ProgramTestContext};
use solana_sdk::{
    signature::Signer,
    signer::keypair::Keypair,
    system_instruction,
    sysvar::clock::Clock,
    transaction::{Transaction, TransactionError},
};
use std::marker::PhantomData;
use std::time::{SystemTime, UNIX_EPOCH};
use test_context::AsyncTestContext;

pub const WALLET_DEFAULT_LAMPORTS: u64 = 10000000;
pub const WALLET_DEFAULT_TOKENS: u64 = 1000;

pub const DEFAULT_PERIOD_DURATION: u32 = 100;

pub enum PeriodType {
    Finality = 0,
    Withdrawal = 1,
    PublicWithdrawal = 2,
    Cancellation = 3,
    PublicCancellation = 4,
}

pub const DEFAULT_ESCROW_AMOUNT: u64 = 100;
pub const DEFAULT_RESCUE_AMOUNT: u64 = 100;
pub const DEFAULT_SAFETY_DEPOSIT: u64 = 25;

pub struct TestArgs {
    pub escrow_amount: u64,
    pub safety_deposit: u64,
    pub finality_duration: u32,
    pub withdrawal_duration: u32,
    pub public_withdrawal_duration: u32,
    pub cancellation_duration: u32,
    pub src_cancellation_timestamp: u32,
    pub init_timestamp: u32,
    pub rescue_start: u32,
    pub rescue_amount: u64,
}

pub fn get_default_testargs(nowsecs: u32) -> TestArgs {
    TestArgs {
        escrow_amount: DEFAULT_ESCROW_AMOUNT,
        safety_deposit: DEFAULT_SAFETY_DEPOSIT,
        finality_duration: DEFAULT_PERIOD_DURATION,
        withdrawal_duration: DEFAULT_PERIOD_DURATION,
        public_withdrawal_duration: DEFAULT_PERIOD_DURATION,
        cancellation_duration: DEFAULT_PERIOD_DURATION,
        src_cancellation_timestamp: nowsecs + 10000,
        init_timestamp: nowsecs,
        rescue_start: nowsecs + RESCUE_DELAY + 100,
        rescue_amount: DEFAULT_RESCUE_AMOUNT,
    }
}

// The phantom type argument is supposed to encode different test state initialization logic for
// src and dst variants.
pub struct TestStateBase<T: ?Sized> {
    pub context: ProgramTestContext,
    pub client: BanksClient,
    pub secret: [u8; 32],
    pub order_hash: Hash,
    pub hashlock: Hash,
    pub token: Pubkey,
    pub payer_kp: Keypair,
    pub creator_wallet: Wallet,
    pub recipient_wallet: Wallet,
    pub test_arguments: TestArgs,
    pub init_timestamp: u32,
    pub pd: PhantomData<T>,
}

// A trait that is used to specify procedures during testing, that
// has to be different between variants.
pub trait EscrowVariant {
    fn get_program_spec() -> (Pubkey, Option<BuiltinFunctionWithContext>);

    // All the instruction creation procedures differ slightly
    // between the variants.
    fn get_create_tx(
        test_state: &TestStateBase<Self>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction;
    fn get_withdraw_tx(
        test_state: &TestStateBase<Self>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction;
    fn get_public_withdraw_tx(
        test_state: &TestStateBase<Self>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
        safety_deposit_recipient: &Keypair,
    ) -> Transaction;
    fn get_cancel_tx(
        test_state: &TestStateBase<Self>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction;
    fn get_rescue_funds_tx(
        test_state: &TestStateBase<Self>,
        escrow: &Pubkey,
        token_to_rescue: &Pubkey,
        escrow_ata: &Pubkey,
        recipient_ata: &Pubkey,
    ) -> Transaction;

    fn get_escrow_data_len() -> usize;
}

impl<T> AsyncTestContext for TestStateBase<T>
where
    T: EscrowVariant,
{
    async fn setup() -> TestStateBase<T> {
        let mut program_test: ProgramTest = ProgramTest::default();
        add_program_to_test(&mut program_test, "escrow_contract", T::get_program_spec);
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
        TestStateBase {
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
        }
    }
}

pub struct Wallet {
    pub keypair: Keypair,
    pub token_account: Pubkey,
}

impl Clone for Wallet {
    fn clone(&self) -> Self {
        Wallet {
            keypair: self.keypair.insecure_clone(),
            token_account: self.token_account,
        }
    }
}

pub fn get_escrow_addresses<T: EscrowVariant>(
    test_state: &TestStateBase<T>,
    creator: Pubkey,
) -> (Pubkey, Pubkey) {
    let (program_id, _) = T::get_program_spec();
    let (escrow_pda, _) = Pubkey::find_program_address(
        &[
            b"escrow",
            test_state.order_hash.as_ref(),
            test_state.hashlock.as_ref(),
            creator.as_ref(),
            test_state.recipient_wallet.keypair.pubkey().as_ref(),
            test_state.token.as_ref(),
            test_state
                .test_arguments
                .escrow_amount
                .to_be_bytes()
                .as_ref(),
            test_state
                .test_arguments
                .safety_deposit
                .to_be_bytes()
                .as_ref(),
            test_state
                .test_arguments
                .rescue_start
                .to_be_bytes()
                .as_ref(),
        ],
        &program_id,
    );
    let escrow_ata = get_associated_token_address(&escrow_pda, &test_state.token);

    (escrow_pda, escrow_ata)
}

pub fn create_escrow_data<T: EscrowVariant>(
    test_state: &TestStateBase<T>,
) -> (Pubkey, Pubkey, Transaction) {
    let (escrow_pda, escrow_ata) =
        get_escrow_addresses(test_state, test_state.creator_wallet.keypair.pubkey());
    let transaction: Transaction = T::get_create_tx(test_state, &escrow_pda, &escrow_ata);

    (escrow_pda, escrow_ata, transaction)
}

pub async fn create_escrow<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) -> (Pubkey, Pubkey) {
    let (escrow, escrow_ata, tx) = create_escrow_data(test_state);
    test_state
        .client
        .process_transaction(tx)
        .await
        .expect_success();
    (escrow, escrow_ata)
}

pub async fn create_wallet(
    ctx: &mut ProgramTestContext,
    token: &Pubkey,
    fund_lamports: u64,
    mint_tokens: u64,
) -> Wallet {
    let dummy_kp = Keypair::new();
    let ata = initialize_spl_associated_account(ctx, token, &dummy_kp.pubkey()).await;
    mint_spl_tokens(ctx, token, &ata, mint_tokens).await;
    transfer_lamports(
        ctx,
        fund_lamports,
        &ctx.payer.insecure_clone(),
        &dummy_kp.pubkey(),
    )
    .await;
    Wallet {
        keypair: dummy_kp,
        token_account: ata,
    }
}

pub fn add_program_to_test<F>(
    program_test: &mut ProgramTest,
    program_name: &'static str,
    get_program_spec: F,
) where
    F: Fn() -> (Pubkey, Option<BuiltinFunctionWithContext>),
{
    let (program_id, entry_point) = get_program_spec();
    program_test.add_program(program_name, program_id, entry_point);
}

pub fn set_time(ctx: &mut ProgramTestContext, timestamp: u32) {
    ctx.set_sysvar(&Clock {
        unix_timestamp: timestamp as i64,
        ..Default::default()
    });
}

pub async fn mint_spl_tokens(
    ctx: &mut ProgramTestContext,
    mint_pk: &Pubkey,
    dst: &Pubkey,
    amount: u64,
) {
    let transfer_ix = spl_instruction::mint_to(
        &spl_program_id,
        mint_pk,
        dst,
        &ctx.payer.pubkey(),
        &[&ctx.payer.pubkey()],
        amount,
    )
    .unwrap();

    let signers: Vec<&Keypair> = vec![&ctx.payer];
    let client = &mut ctx.banks_client;
    client
        .process_transaction(Transaction::new_signed_with_payer(
            &[transfer_ix],
            Some(&ctx.payer.pubkey()),
            &signers,
            ctx.last_blockhash,
        ))
        .await
        .unwrap();
}

pub async fn transfer_lamports(
    ctx: &mut ProgramTestContext,
    amount: u64,
    src: &Keypair,
    dst: &Pubkey,
) {
    let transfer_ix = system_instruction::transfer(&src.pubkey(), dst, amount);
    let signers: Vec<&Keypair> = vec![src];
    let client = &mut ctx.banks_client;
    client
        .process_transaction(Transaction::new_signed_with_payer(
            &[transfer_ix],
            Some(&ctx.payer.pubkey()),
            &signers,
            ctx.last_blockhash,
        ))
        .await
        .unwrap();
}

pub async fn deploy_spl_token(ctx: &mut ProgramTestContext, decimals: u8) -> Keypair {
    // create mint account
    let mint_keypair = Keypair::new();
    let create_mint_acc_ix = system_instruction::create_account(
        &ctx.payer.pubkey(),
        &mint_keypair.pubkey(),
        1_000_000_000, // Some lamports to pay rent
        Mint::LEN as u64,
        &spl_program_id,
    );

    // initialize mint account
    let initialize_mint_ix: Instruction = spl_instruction::initialize_mint(
        &spl_program_id,
        &mint_keypair.pubkey(),
        &ctx.payer.pubkey(),
        Option::None,
        decimals,
    )
    .unwrap();

    let signers: Vec<&Keypair> = vec![&ctx.payer, &mint_keypair];

    let client = &mut ctx.banks_client;
    client
        .process_transaction(Transaction::new_signed_with_payer(
            &[create_mint_acc_ix, initialize_mint_ix],
            Some(&ctx.payer.pubkey()),
            &signers,
            ctx.last_blockhash,
        ))
        .await
        .unwrap();
    mint_keypair
}

pub async fn get_token_balance(ctx: &mut ProgramTestContext, account: &Pubkey) -> u64 {
    let client = &mut ctx.banks_client;
    let spl_account: SplTokenAccount = client
        .get_packed_account_data::<SplTokenAccount>(*account)
        .await
        .unwrap();
    spl_account.amount
}

pub async fn initialize_spl_associated_account(
    ctx: &mut ProgramTestContext,
    mint_pubkey: &Pubkey,
    account: &Pubkey,
) -> Pubkey {
    let ata = get_associated_token_address(account, mint_pubkey);
    let create_spl_acc_ix =
        create_associated_token_account(&ctx.payer.pubkey(), account, mint_pubkey, &spl_program_id);

    let signers: Vec<&Keypair> = vec![&ctx.payer];

    let client = &mut ctx.banks_client;
    client
        .process_transaction(Transaction::new_signed_with_payer(
            &[create_spl_acc_ix],
            Some(&ctx.payer.pubkey()),
            &signers,
            ctx.last_blockhash,
        ))
        .await
        .unwrap();
    ata
}

#[derive(Clone)]
pub enum BalanceChange {
    Token(Pubkey, i128),
    Native(Pubkey, i128),
}

pub fn native_change(k: Pubkey, d: u64) -> BalanceChange {
    BalanceChange::Native(k, d as i128)
}

pub fn token_change(k: Pubkey, d: u64) -> BalanceChange {
    BalanceChange::Token(k, d as i128)
}

async fn get_balances<T>(
    test_state: &mut TestStateBase<T>,
    balance_query: &[BalanceChange],
) -> Vec<u64> {
    let mut result: Vec<u64> = vec![];
    for b in balance_query {
        match b {
            BalanceChange::Token(k, _) => {
                result.push(get_token_balance(&mut test_state.context, k).await)
            }
            BalanceChange::Native(k, _) => {
                result.push(test_state.client.get_balance(*k).await.unwrap())
            }
        }
    }
    result
}

impl<T> TestStateBase<T> {
    pub async fn expect_balance_change(&mut self, tx: Transaction, diff: &[BalanceChange]) {
        let balances_before = get_balances(self, diff).await;

        // execute transaction
        self.client.process_transaction(tx).await.expect_success();

        // compare balances
        let balances_after = get_balances(self, diff).await;
        for ((before, after), exp) in balances_before
            .iter()
            .zip(balances_after.iter())
            .zip(diff.iter())
        {
            let real_diff: i128 = *after as i128 - *before as i128;
            match exp {
                BalanceChange::Token(k, token_expected_diff) => {
                    assert_eq!(
                        real_diff, *token_expected_diff,
                        "Token balance changed unexpectedley for {}, left = {}, right = {}",
                        k, real_diff, token_expected_diff
                    )
                }
                BalanceChange::Native(k, native_expected_diff) => {
                    assert_eq!(
                        real_diff, *native_expected_diff,
                        "SOL balance changed unexpectedley for {}, left = {}, right = {}",
                        k, real_diff, native_expected_diff
                    )
                }
            }
        }
    }
}

pub trait Expectation {
    type ExpectationType;
    fn expect_success(self);
    fn expect_error(self, expectation: Self::ExpectationType);
}

impl Expectation for Result<(), BanksClientError> {
    type ExpectationType = (u8, ProgramError);
    fn expect_success(self) {
        self.unwrap()
    }
    fn expect_error(self, expectation: (u8, ProgramError)) {
        let (index, expected_program_error) = expectation;
        if let TransactionError::InstructionError(result_instr_idx, result_instr_error) = self
            .expect_err("Expected an error, but transaction succeeded")
            .unwrap()
        {
            let result_program_error: ProgramError = result_instr_error.try_into().unwrap();
            assert_eq!(
                (index, expected_program_error),
                (result_instr_idx, result_program_error)
            );
        } else {
            panic!("Unexpected error provided: {:?}", expected_program_error);
        }
    }
}

pub async fn get_min_rent_for_size(client: &mut BanksClient, s: usize) -> u64 {
    let rent = client.get_rent().await.unwrap();
    rent.minimum_balance(s)
}

pub fn get_token_account_size() -> usize {
    SplTokenAccount::LEN
}

// This wrapper is used to coerce (unsafely so) the entry function generated by
// anchor, to a more general (w.r.t lifetimes) type that is required by the `processor` macro.
// Specifically the lifetime bound on the accounts argument for the function that the `processor!`
// macro expects is `&'a [AccountInfo<'info>]`, but the anchor generated entry point requires
// `&'a[AccountInfo<'a>]`, so we wrap the anchor entrypoint in a function that accept a
// `&'a[AccountInfo<'info>]` and usafely coerce into `&'a [AccountInfo<'a>]`. How safe this is
// needs to be investigated, for now, it appear to work.
//
// We have to do this because the alternate way only appear to work in sbf builds, but the code
// coverage analyis infra requires the project buildable using `cargo test`. See the doc linked
// here, https://github.com/LimeChain/zest?tab=readme-ov-file#compatibility-requirements
#[macro_export]
macro_rules! wrap_entry {
    ($entry: expr) => {
        processor!(
            |program_id: &Pubkey, accounts: &[AccountInfo], instruction_data: &[u8]| {
                $entry(
                    program_id,
                    unsafe {
                        std::mem::transmute::<&[AccountInfo<'_>], &[AccountInfo<'_>]>(accounts)
                    },
                    instruction_data,
                )
            }
        )
    };
}
