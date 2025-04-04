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
use solana_program_test::{
    BanksClient, BanksClientError, BanksTransactionResultWithMetadata, ProgramTest,
    ProgramTestContext,
};
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

pub const ERROR_INSUFFICIENT_FUNDS: &str = "Error: insufficient funds";
pub const ERROR_ALREADY_USED: &str = "already in use";
pub const ERROR_CONSTRAINT_TOKENOWNER: &str = "Error Code: ConstraintTokenOwner";
pub const ERROR_CONSTRAINT_ASSOCIATED: &str = "Error Code: ConstraintAssociated";
pub const ERROR_CONSTRAINT_SEEDS: &str = "Error Code: ConstraintSeeds";
pub const ERROR_INVALID_TIME: &str = "Error Code: InvalidTime";

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

fn get_default_testargs(nowsecs: u32) -> TestArgs {
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
    pd: PhantomData<T>,
}

// A trait that is used to specify procedures during testing, that
// has to be different between variants.
pub trait EscrowVariant {
    fn get_program_spec() -> (Pubkey, Option<BuiltinFunctionWithContext>);

    // Required because withdraw transaction needs to be
    // signed differently in src and dst variants.
    fn withdraw_ix_to_signed_tx(ix: Instruction, test_state: &TestStateBase<Self>) -> Transaction;

    // All the instruction creation procedures differ slightly
    // between the variants.
    fn get_public_withdraw_ix(
        test_state: &TestStateBase<Self>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
        safety_deposit_recipient: Pubkey,
        recipient_ata_override: Option<Pubkey>,
        secret_override: Option<[u8; 32]>,
    ) -> Instruction;
    fn get_withdraw_ix(
        test_state: &TestStateBase<Self>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
        recipient_ata_override: Option<Pubkey>,
        secret_override: Option<[u8; 32]>,
    ) -> Instruction;
    fn get_cancel_ix(
        test_state: &TestStateBase<Self>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
        canceller_override: Option<Wallet>,
    ) -> Instruction;
    fn get_create_ix(
        test_state: &TestStateBase<Self>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Instruction;
    fn get_rescue_funds_ix(
        test_state: &TestStateBase<Self>,
        escrow: &Pubkey,
        token_to_rescue: &Pubkey,
        escrow_ata: &Pubkey,
        recipient_wallet_override: Option<Wallet>,
    ) -> Instruction;

    fn get_escrow_data_len() -> usize;
}

impl<T> AsyncTestContext for TestStateBase<T>
where
    T: EscrowVariant,
{
    async fn setup() -> TestStateBase<T> {
        let (program_id, entry_point) = T::get_program_spec();
        let mut context: ProgramTestContext =
            start_context("escrow_contract", program_id, entry_point).await;
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

pub fn create_escrow_data<T: EscrowVariant>(
    test_state: &TestStateBase<T>,
) -> (Pubkey, Pubkey, Instruction) {
    let (program_id, _) = T::get_program_spec();
    let (escrow_pda, _) = Pubkey::find_program_address(
        &[
            b"escrow",
            test_state.order_hash.as_ref(),
            test_state.hashlock.as_ref(),
            test_state.creator_wallet.keypair.pubkey().as_ref(),
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

    let instruction: Instruction = T::get_create_ix(test_state, &escrow_pda, &escrow_ata);

    (escrow_pda, escrow_ata, instruction)
}

pub async fn create_escrow_tx<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) -> (Pubkey, Pubkey, Result<(), BanksClientError>) {
    let mut client = test_state.context.banks_client.clone();
    let (escrow, escrow_ata, create_ix) = create_escrow_data(test_state);

    let transaction = Transaction::new_signed_with_payer(
        &[create_ix],
        Some(&test_state.payer_kp.pubkey()),
        &[
            &test_state.context.payer,
            &test_state.creator_wallet.keypair,
        ],
        test_state.context.last_blockhash,
    );

    (
        escrow,
        escrow_ata,
        client.process_transaction(transaction).await,
    )
}

pub async fn create_escrow<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) -> (Pubkey, Pubkey) {
    let (escrow, escrow_ata, tx) = create_escrow_tx(test_state).await;
    tx.expect_success();
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

pub async fn start_context(
    contract_name: &str,
    program_id: Pubkey,
    entry_point: Option<BuiltinFunctionWithContext>,
) -> ProgramTestContext {
    let program_test = ProgramTest::new(contract_name, program_id, entry_point);
    program_test.start_with_context().await
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

impl<T> TestStateBase<T> {
    pub async fn expect_err_in_tx_meta(&mut self, mut tx: Transaction, expectation: &str) {
        // retry at most 5 times.
        for _ in 0..5 {
            let r = self
                .client
                .process_transaction_with_metadata(tx.clone())
                .await;
            match r {
                Result::Ok(BanksTransactionResultWithMetadata {
                    metadata: Some(m), ..
                }) => {
                    return assert!(m.log_messages.iter().any(|x| x.contains(expectation)));
                }
                _ => {
                    let new_hash = self.context.get_new_latest_blockhash().await.unwrap();
                    tx.message.recent_blockhash = new_hash;
                }
            }
        }
        panic!("Failed to fetch transaction metadata!")
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
                    unsafe { std::mem::transmute(accounts) },
                    instruction_data,
                )
            }
        )
    };
}
