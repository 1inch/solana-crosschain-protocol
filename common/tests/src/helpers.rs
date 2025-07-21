use anchor_lang::Space;
use anchor_spl::associated_token::{
    spl_associated_token_account, spl_associated_token_account::instruction as spl_ata_instruction,
};
use anchor_spl::token::spl_token::{
    instruction::{self as spl_instruction, sync_native},
    native_mint::ID as NATIVE_MINT,
    state::{Account as SplTokenAccount, Mint},
    ID as spl_program_id,
};
use anchor_spl::token_2022::spl_token_2022::{
    extension::ExtensionType, extension::StateWithExtensionsMut,
    instruction as spl2022_instruction, state::Account as SplToken2022Account,
    state::Mint as SPL2022_Mint, ID as spl2022_program_id,
};

use async_trait::async_trait;
use common::timelocks::{Stage, Timelocks};
use cross_chain_escrow_src::DstChainParams;
use cross_chain_escrow_src::{get_escrow_hashlock, merkle_tree::MerkleProof};
use primitive_types::U256;
use solana_program::{
    instruction::Instruction,
    keccak::{hashv, Hash},
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
};
use solana_program_runtime::invoke_context::BuiltinFunctionWithContext;
use solana_program_test::{
    BanksClient, BanksClientError, ProgramTest, ProgramTestBanksClientExt, ProgramTestContext,
};
use solana_sdk::{
    native_token::LAMPORTS_PER_SOL,
    signature::Signer,
    signer::keypair::Keypair,
    system_instruction,
    sysvar::clock::Clock,
    transaction::{Transaction, TransactionError},
};
use std::marker::PhantomData;
use std::ops::{Div, Mul};
use std::time::{SystemTime, UNIX_EPOCH};
use test_context::AsyncTestContext;

use crate::whitelist::get_program_whitelist_spec;

pub const DEFAULT_FEE_PER_SIGNATURE_LAMPORTS: u64 = 5000;

pub const WALLET_DEFAULT_LAMPORTS: u64 = 10 * LAMPORTS_PER_SOL;
pub const WALLET_DEFAULT_TOKENS: u64 = 1000000000;

pub const DEFAULT_PERIOD_DURATION: u32 = 100;
pub const DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE: u64 = 4;

pub const DEFAULT_ESCROW_AMOUNT: u64 = 100000;
pub const DEFAULT_DST_ESCROW_AMOUNT: u64 = 1000;
pub const DEFAULT_RESCUE_AMOUNT: u64 = 100;
pub const DEFAULT_SAFETY_DEPOSIT: u64 = 25;
pub const DEFAULT_SALT: u64 = 0xFACE8D00DEADBEEF;

pub const DEFAULT_SRC_ESCROW_SIZE: usize = cross_chain_escrow_src::EscrowSrc::INIT_SPACE
    + cross_chain_escrow_src::constants::DISCRIMINATOR_BYTES;

pub const DEFAULT_DST_ESCROW_SIZE: usize = cross_chain_escrow_dst::EscrowDst::INIT_SPACE
    + cross_chain_escrow_dst::constants::DISCRIMINATOR_BYTES;

pub const DEFAULT_ORDER_SIZE: usize = cross_chain_escrow_src::constants::DISCRIMINATOR_BYTES
    + cross_chain_escrow_src::Order::INIT_SPACE;

pub fn init_timelocks(
    src_withdrawal: u32,
    src_public_withdrawal: u32,
    src_cancellation: u32,
    src_public_cancellation: u32,
    dst_withdrawal: u32,
    dst_public_withdrawal: u32,
    dst_cancellation: u32,
    deployed_at: u32,
) -> Timelocks {
    const DEPLOYED_AT_OFFSET: usize = 224;
    const STAGE_BIT_SIZE: usize = 32;

    let mut value = U256::zero();

    value |= U256::from(deployed_at) << DEPLOYED_AT_OFFSET;
    value |= U256::from(src_withdrawal) << ((Stage::SrcWithdrawal as usize) * STAGE_BIT_SIZE);
    value |= U256::from(src_public_withdrawal)
        << ((Stage::SrcPublicWithdrawal as usize) * STAGE_BIT_SIZE);
    value |= U256::from(src_cancellation) << ((Stage::SrcCancellation as usize) * STAGE_BIT_SIZE);
    value |= U256::from(src_public_cancellation)
        << ((Stage::SrcPublicCancellation as usize) * STAGE_BIT_SIZE);
    value |= U256::from(dst_withdrawal) << ((Stage::DstWithdrawal as usize) * STAGE_BIT_SIZE);
    value |= U256::from(dst_public_withdrawal)
        << ((Stage::DstPublicWithdrawal as usize) * STAGE_BIT_SIZE);
    value |= U256::from(dst_cancellation) << ((Stage::DstCancellation as usize) * STAGE_BIT_SIZE);

    Timelocks(value)
}

pub struct TestArgs {
    pub order_amount: u64,
    pub order_remaining_amount: u64,
    pub escrow_amount: u64,
    pub safety_deposit: u64,
    pub src_timelocks: Timelocks,
    pub dst_timelocks: Timelocks,
    pub src_cancellation_timestamp: u32,
    pub init_timestamp: u32,
    pub rescue_amount: u64,
    pub expiration_time: u32,
    pub asset_is_native: bool,
    pub dst_amount: [u64; 4],
    pub dutch_auction_data: cross_chain_escrow_src::AuctionData,
    pub max_cancellation_premium: u64,
    pub cancellation_auction_duration: u32,
    pub reward_limit: u64,
    pub merkle_proof: Option<MerkleProof>,
    pub merkle_root: Hash,
    pub allow_multiple_fills: bool,
    pub dst_chain_params: DstChainParams,
    pub salt: u64,
    pub partial_secrets: Vec<[u8; 32]>,
}

pub fn get_default_testargs(nowsecs: u32) -> TestArgs {
    TestArgs {
        order_amount: DEFAULT_ESCROW_AMOUNT,
        order_remaining_amount: DEFAULT_ESCROW_AMOUNT,
        escrow_amount: DEFAULT_ESCROW_AMOUNT,
        safety_deposit: DEFAULT_SAFETY_DEPOSIT,
        src_timelocks: init_timelocks(
            DEFAULT_PERIOD_DURATION,
            DEFAULT_PERIOD_DURATION * 2,
            DEFAULT_PERIOD_DURATION * 3,
            DEFAULT_PERIOD_DURATION * 4,
            DEFAULT_PERIOD_DURATION,
            DEFAULT_PERIOD_DURATION * 2,
            DEFAULT_PERIOD_DURATION * 3,
            nowsecs,
        ),
        dst_timelocks: init_timelocks(
            0,
            0,
            0,
            0,
            DEFAULT_PERIOD_DURATION,
            DEFAULT_PERIOD_DURATION * 2,
            DEFAULT_PERIOD_DURATION * 3,
            nowsecs,
        ),
        src_cancellation_timestamp: nowsecs + 10000,
        init_timestamp: nowsecs,
        rescue_amount: DEFAULT_RESCUE_AMOUNT,
        expiration_time: nowsecs + DEFAULT_PERIOD_DURATION,
        asset_is_native: false, // This is set to false by default, will be changed for native tests.
        dst_amount: U256::from(DEFAULT_DST_ESCROW_AMOUNT).0,
        dutch_auction_data: cross_chain_escrow_src::AuctionData {
            start_time: nowsecs,
            duration: DEFAULT_PERIOD_DURATION,
            initial_rate_bump: 0,
            points_and_time_deltas: vec![],
        },
        max_cancellation_premium: DEFAULT_ESCROW_AMOUNT.mul(50_u64 * 100).div(100_u64 * 100),
        cancellation_auction_duration: DEFAULT_PERIOD_DURATION,
        reward_limit: DEFAULT_ESCROW_AMOUNT.mul(50_u64 * 100).div(100_u64 * 100),
        merkle_proof: None,
        merkle_root: Hash::default(),
        allow_multiple_fills: false,
        salt: DEFAULT_SALT,
        dst_chain_params: DstChainParams {
            chain_id: 0u32,
            maker_address: [0u8; 32],
            token: [0u8; 32],
            safety_deposit: DEFAULT_SAFETY_DEPOSIT as u128,
        },
        partial_secrets: Vec::new(),
    }
}

// The phantom type argument is supposed to encode different test state initialization logic for
// src and dst variants.

pub struct TestStateBase<T: ?Sized, S: ?Sized> {
    pub context: ProgramTestContext,
    pub client: BanksClient,
    pub secret: [u8; 32],
    pub order_hash: Hash,
    pub hashlock: Hash,
    pub token: Pubkey,
    pub payer_kp: Keypair,
    pub authority_whitelist_kp: Keypair,
    pub maker_wallet: Wallet,
    pub taker_wallet: Wallet,
    pub test_arguments: TestArgs,
    pub init_timestamp: u32,
    pub pd: (PhantomData<T>, PhantomData<S>),
}

#[async_trait]
pub trait TokenVariant {
    fn get_token_program_id() -> Pubkey;
    fn get_token_account_size() -> usize;
    async fn deploy_spl_token(context: &mut ProgramTestContext) -> Keypair;
    async fn initialize_spl_associated_account(
        context: &mut ProgramTestContext,
        mint_pk: &Pubkey,
        owner: &Pubkey,
    ) -> Pubkey;
    async fn mint_spl_tokens(
        ctx: &mut ProgramTestContext,
        mint_pk: &Pubkey,
        dst: &Pubkey,
        owner: &Pubkey,
        signer: &Keypair,
        amount: u64,
    );
    async fn burn_tokens(
        ctx: &mut ProgramTestContext,
        source_ata: &Pubkey,
        mint: &Pubkey,
        authority: &Keypair,
        signer: &Keypair,
        amount: u64,
    );
    async fn close_ata(
        ctx: &mut ProgramTestContext,
        closing_account: &Pubkey,
        dst: &Pubkey,
        owner: &Pubkey,
        signer: &Keypair,
    );
}

pub struct TokenSPL;
pub struct Token2022;

#[async_trait]
impl TokenVariant for Token2022 {
    fn get_token_account_size() -> usize {
        // Compute account size with immutable owner extension enabled, as done by the Assocaited
        // Token Account program.
        //
        //https://github.com/solana-program/associated-token-account/blob/main/program/src/processor.rs#L121
        ExtensionType::try_calculate_account_len::<SplToken2022Account>(&[
            ExtensionType::ImmutableOwner,
        ])
        .unwrap()
    }

    fn get_token_program_id() -> Pubkey {
        spl2022_program_id
    }

    async fn deploy_spl_token(ctx: &mut ProgramTestContext) -> Keypair {
        // create mint account
        let mint_keypair = Keypair::new();
        let account_size = ExtensionType::try_calculate_account_len::<SPL2022_Mint>(&[]).unwrap();
        let create_mint_acc_ix = system_instruction::create_account(
            &ctx.payer.pubkey(),
            &mint_keypair.pubkey(),
            1_000_000_000,
            account_size as u64,
            &spl2022_program_id,
        );

        // initialize mint account
        let initialize_mint_ix: Instruction = spl2022_instruction::initialize_mint(
            &spl2022_program_id,
            &mint_keypair.pubkey(),
            &ctx.payer.pubkey(),
            None,
            8,
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

    async fn initialize_spl_associated_account(
        ctx: &mut ProgramTestContext,
        mint_pubkey: &Pubkey,
        account: &Pubkey,
    ) -> Pubkey {
        let ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            account,
            mint_pubkey,
            &spl2022_program_id,
        );
        let create_spl_acc_ix = spl_ata_instruction::create_associated_token_account(
            &ctx.payer.pubkey(),
            account,
            mint_pubkey,
            &spl2022_program_id,
        );

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

    async fn mint_spl_tokens(
        ctx: &mut ProgramTestContext,
        mint_pk: &Pubkey,
        dst: &Pubkey,
        owner: &Pubkey,
        signer: &Keypair,
        amount: u64,
    ) {
        let transfer_ix = spl2022_instruction::mint_to(
            &spl2022_program_id,
            mint_pk,
            dst,
            owner, // mint authority, which should be ctx.payer.
            &[&signer.pubkey()],
            amount,
        )
        .unwrap();
        let signers: Vec<&Keypair> = vec![signer];
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

    async fn burn_tokens(
        ctx: &mut ProgramTestContext,
        source_ata: &Pubkey,
        mint: &Pubkey,
        authority: &Keypair,
        signer: &Keypair,
        amount: u64,
    ) {
        let burn_ix = spl2022_instruction::burn(
            &spl2022_program_id,
            source_ata,
            mint,
            &authority.pubkey(),
            &[&authority.pubkey(), &signer.pubkey()],
            amount,
        )
        .unwrap();

        let signers: Vec<&Keypair> = vec![authority, signer];
        let client = &mut ctx.banks_client;
        client
            .process_transaction(Transaction::new_signed_with_payer(
                &[burn_ix],
                Some(&ctx.payer.pubkey()),
                &signers,
                ctx.last_blockhash,
            ))
            .await
            .unwrap();
    }

    async fn close_ata(
        ctx: &mut ProgramTestContext,
        closing_account: &Pubkey,
        dst: &Pubkey,
        owner: &Pubkey,
        signer: &Keypair,
    ) {
        let close_ix = spl2022_instruction::close_account(
            &spl2022_program_id,
            closing_account,
            dst,
            owner,
            &[&signer.pubkey()],
        )
        .unwrap();
        let client = &mut ctx.banks_client;
        client
            .process_transaction(Transaction::new_signed_with_payer(
                &[close_ix],
                Some(&ctx.payer.pubkey()),
                &[&ctx.payer, signer],
                ctx.last_blockhash,
            ))
            .await
            .unwrap();
    }
}

#[async_trait]
impl TokenVariant for TokenSPL {
    fn get_token_program_id() -> Pubkey {
        spl_program_id
    }
    fn get_token_account_size() -> usize {
        SplTokenAccount::LEN
    }
    async fn deploy_spl_token(ctx: &mut ProgramTestContext) -> Keypair {
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
            8,
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

    async fn initialize_spl_associated_account(
        ctx: &mut ProgramTestContext,
        mint_pubkey: &Pubkey,
        account: &Pubkey,
    ) -> Pubkey {
        let ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            account,
            mint_pubkey,
            &spl_program_id,
        );
        let create_spl_acc_ix = spl_ata_instruction::create_associated_token_account(
            &ctx.payer.pubkey(),
            account,
            mint_pubkey,
            &spl_program_id,
        );

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

    async fn mint_spl_tokens(
        ctx: &mut ProgramTestContext,
        mint_pk: &Pubkey,
        dst: &Pubkey,
        owner: &Pubkey,
        signer: &Keypair,
        amount: u64,
    ) {
        let transfer_ix = spl_instruction::mint_to(
            &spl_program_id,
            mint_pk,
            dst,
            owner, // mint authority, which should be ctx.payer.
            &[&signer.pubkey()],
            amount,
        )
        .unwrap();
        let signers: Vec<&Keypair> = vec![signer];
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

    async fn burn_tokens(
        ctx: &mut ProgramTestContext,
        source_ata: &Pubkey,
        mint: &Pubkey,
        authority: &Keypair,
        signer: &Keypair,
        amount: u64,
    ) {
        let burn_ix = spl_instruction::burn(
            &spl_program_id,
            source_ata,
            mint,
            &authority.pubkey(),
            &[&authority.pubkey(), &signer.pubkey()],
            amount,
        )
        .unwrap();

        let signers: Vec<&Keypair> = vec![authority, signer];
        let client = &mut ctx.banks_client;
        client
            .process_transaction(Transaction::new_signed_with_payer(
                &[burn_ix],
                Some(&ctx.payer.pubkey()),
                &signers,
                ctx.last_blockhash,
            ))
            .await
            .unwrap();
    }

    async fn close_ata(
        ctx: &mut ProgramTestContext,
        closing_account: &Pubkey,
        dst: &Pubkey,
        owner: &Pubkey,
        signer: &Keypair,
    ) {
        let close_ix = spl_instruction::close_account(
            &spl_program_id,
            closing_account,
            dst,
            owner,
            &[&signer.pubkey()],
        )
        .unwrap();
        let client = &mut ctx.banks_client;
        client
            .process_transaction(Transaction::new_signed_with_payer(
                &[close_ix],
                Some(&ctx.payer.pubkey()),
                &[&ctx.payer, signer],
                ctx.last_blockhash,
            ))
            .await
            .unwrap();
    }
}

// A trait that is used to specify procedures during testing, that
// has to be different between variants.
pub trait EscrowVariant<S: TokenVariant> {
    fn get_program_spec() -> (Pubkey, Option<BuiltinFunctionWithContext>);

    // All the instruction creation procedures differ slightly
    // between the variants.
    fn get_create_tx(
        test_state: &TestStateBase<Self, S>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction;
    fn get_withdraw_tx(
        test_state: &TestStateBase<Self, S>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction;
    fn get_public_withdraw_tx(
        test_state: &TestStateBase<Self, S>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
        safety_deposit_recipient: &Keypair,
    ) -> Transaction;
    fn get_cancel_tx(
        test_state: &TestStateBase<Self, S>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction;
    fn get_rescue_funds_tx(
        test_state: &TestStateBase<Self, S>,
        escrow: &Pubkey,
        token_to_rescue: &Pubkey,
        escrow_ata: &Pubkey,
        taker_ata: &Pubkey,
    ) -> Transaction;

    fn get_escrow_data_len() -> usize;
}

impl<T, S> AsyncTestContext for TestStateBase<T, S>
where
    T: EscrowVariant<S>,
    S: TokenVariant,
{
    async fn setup() -> TestStateBase<T, S> {
        let mut program_test: ProgramTest = ProgramTest::default();
        add_program_to_test(&mut program_test, "escrow_contract", T::get_program_spec);
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
        let token = S::deploy_spl_token(&mut context).await.pubkey();
        let secret = hashv(&[b"default_secret"]).to_bytes();
        let payer_kp = context.payer.insecure_clone();
        let authority_whitelist_kp = Keypair::new();
        transfer_lamports(
            &mut context,
            WALLET_DEFAULT_LAMPORTS,
            &payer_kp,
            &authority_whitelist_kp.pubkey(),
        )
        .await;
        let maker_wallet = create_wallet::<S>(
            &mut context,
            &token,
            &payer_kp,
            &payer_kp,
            WALLET_DEFAULT_LAMPORTS,
            WALLET_DEFAULT_TOKENS,
        )
        .await;
        let taker_wallet = create_wallet::<S>(
            &mut context,
            &token,
            &payer_kp,
            &payer_kp,
            WALLET_DEFAULT_LAMPORTS,
            WALLET_DEFAULT_TOKENS,
        )
        .await;
        TestStateBase {
            context,
            client,
            secret,
            order_hash: Hash::new_unique(),
            hashlock: hashv(&[secret.as_ref()]),
            token,
            payer_kp: payer_kp.insecure_clone(),
            authority_whitelist_kp,
            maker_wallet,
            taker_wallet,
            init_timestamp: timestamp,
            test_arguments: get_default_testargs(timestamp),
            pd: (PhantomData, PhantomData),
        }
    }
}

pub trait HasTokenVariant {
    type Token: TokenVariant;
}

impl<T: EscrowVariant<S>, S: TokenVariant> HasTokenVariant for TestStateBase<T, S> {
    type Token = S;
}

#[derive(Debug)]
pub struct Wallet {
    pub keypair: Keypair,
    pub token_account: Pubkey,
    pub native_token_account: Pubkey,
}

impl Clone for Wallet {
    fn clone(&self) -> Self {
        Wallet {
            keypair: self.keypair.insecure_clone(),
            token_account: self.token_account,
            native_token_account: self.native_token_account,
        }
    }
}

pub fn get_escrow_addresses<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &TestStateBase<T, S>,
    creator: Pubkey,
) -> (Pubkey, Pubkey) {
    let program_id = T::get_program_spec().0;
    let hashlock = get_escrow_hashlock(
        test_state.hashlock.to_bytes(),
        test_state.test_arguments.merkle_proof.clone(),
    );
    let (escrow_pda, _) = Pubkey::find_program_address(
        &[
            b"escrow",
            test_state.order_hash.as_ref(),
            hashlock.as_ref(),
            creator.as_ref(),
            test_state.taker_wallet.keypair.pubkey().as_ref(),
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
        ],
        &program_id,
    );
    let escrow_ata = spl_associated_token_account::get_associated_token_address_with_program_id(
        &escrow_pda,
        &test_state.token,
        &S::get_token_program_id(),
    );

    (escrow_pda, escrow_ata)
}

pub fn create_escrow_data<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &TestStateBase<T, S>,
) -> (Pubkey, Pubkey, Transaction) {
    let (escrow_pda, escrow_ata) =
        get_escrow_addresses(test_state, test_state.maker_wallet.keypair.pubkey());
    let transaction: Transaction = T::get_create_tx(test_state, &escrow_pda, &escrow_ata);

    (escrow_pda, escrow_ata, transaction)
}

pub async fn create_escrow<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &TestStateBase<T, S>,
) -> (Pubkey, Pubkey) {
    let (escrow, escrow_ata, tx) = create_escrow_data(test_state);
    test_state
        .client
        .process_transaction(tx)
        .await
        .expect_success();
    (escrow, escrow_ata)
}

pub async fn create_wallet<S: TokenVariant>(
    ctx: &mut ProgramTestContext,
    token: &Pubkey,
    mint_authority: &Keypair,
    payer: &Keypair,
    fund_lamports: u64,
    mint_tokens: u64,
) -> Wallet {
    let dummy_kp = Keypair::new();
    let ata = S::initialize_spl_associated_account(ctx, token, &dummy_kp.pubkey()).await;
    S::mint_spl_tokens(
        ctx,
        token,
        &ata,
        &mint_authority.pubkey(),
        mint_authority,
        mint_tokens,
    )
    .await;
    transfer_lamports(ctx, fund_lamports, payer, &dummy_kp.pubkey()).await;

    let native_ata =
        TokenSPL::initialize_spl_associated_account(ctx, &NATIVE_MINT, &dummy_kp.pubkey()).await;
    transfer_lamports(ctx, mint_tokens, payer, &native_ata).await;
    sync_native_ata(ctx, &native_ata).await;

    Wallet {
        keypair: dummy_kp,
        token_account: ata,
        native_token_account: native_ata,
    }
}

pub async fn sync_native_ata(ctx: &mut ProgramTestContext, ata: &Pubkey) {
    let ix = sync_native(&spl_program_id, ata).unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer],
        ctx.last_blockhash,
    );

    ctx.banks_client.process_transaction(tx).await.unwrap();
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

pub async fn transfer_lamports(
    ctx: &mut ProgramTestContext,
    amount: u64,
    src: &Keypair,
    dst: &Pubkey,
) {
    let transfer_ix = system_instruction::transfer(&src.pubkey(), dst, amount);
    let signers: Vec<&Keypair> = vec![src];
    // Updating the latest blockhash to avoid the "RpcError(DeadlineExceeded)" error
    let last_blockhash = ctx
        .banks_client
        .get_new_latest_blockhash(&ctx.last_blockhash)
        .await
        .unwrap();
    let client = &mut ctx.banks_client;
    client
        .process_transaction(Transaction::new_signed_with_payer(
            &[transfer_ix],
            Some(&ctx.payer.pubkey()),
            &signers,
            last_blockhash,
        ))
        .await
        .unwrap();
}

pub async fn get_token_balance(ctx: &mut ProgramTestContext, account: &Pubkey) -> u64 {
    let client = &mut ctx.banks_client;
    let mut account_data = client.get_account(*account).await.unwrap().unwrap();
    let state =
        StateWithExtensionsMut::<SplToken2022Account>::unpack(&mut account_data.data).unwrap();
    state.base.amount
}

#[derive(Clone)]
pub enum BalanceChange {
    Token(Pubkey, i128),
    Native(Pubkey, i128),
}

#[derive(Clone)]
pub enum StateChange {
    Balance(BalanceChange),
    ClosedAccount(Pubkey, bool),
}

pub fn native_change(k: Pubkey, d: u64) -> StateChange {
    StateChange::Balance(BalanceChange::Native(k, d as i128))
}

pub fn token_change(k: Pubkey, d: u64) -> StateChange {
    StateChange::Balance(BalanceChange::Token(k, d as i128))
}

pub fn account_closure(k: Pubkey, d: bool) -> StateChange {
    StateChange::ClosedAccount(k, d)
}

async fn get_balances<T, S>(
    test_state: &mut TestStateBase<T, S>,
    balance_query: &[BalanceChange],
) -> Vec<u64> {
    let mut result = Vec::with_capacity(balance_query.len());
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

impl<T, S> TestStateBase<T, S> {
    pub async fn expect_state_change(&mut self, tx: Transaction, diff: &[StateChange]) {
        let mut balance_changes = Vec::new();
        let mut closure_checks = Vec::new();

        for change in diff {
            match change {
                StateChange::Balance(b) => balance_changes.push(b.clone()),
                StateChange::ClosedAccount(p, b) => closure_checks.push((p, b)),
            }
        }

        // Get balances before tx
        let balances_before = get_balances(self, &balance_changes).await;
        // Execute transaction
        self.client.process_transaction(tx).await.expect_success();

        let balances_after = get_balances(self, &balance_changes).await;

        // Assert balance differences
        for ((before, after), exp) in balances_before
            .iter()
            .zip(balances_after.iter())
            .zip(balance_changes.iter())
        {
            let real_diff = *after as i128 - *before as i128;
            match exp {
                BalanceChange::Token(k, expected) => {
                    assert_eq!(
                        real_diff, *expected,
                        "Token balance changed unexpectedly for {}, real = {}, expected = {}, diff = {}",
                        k, real_diff, expected, expected - real_diff
                    );
                }
                BalanceChange::Native(k, expected) => {
                    assert_eq!(
                        real_diff, *expected,
                        "SOL balance changed unexpectedly for {}, real = {}, expected = {}, diff = {}",
                        k, real_diff, expected, expected - real_diff
                    );
                }
            }
        }

        // Assert account closures
        for (account, should_be_closed) in closure_checks {
            let acc = self.client.get_account(*account).await.unwrap();
            if *should_be_closed {
                assert!(
                    acc.is_none(),
                    "Expected account {} to be closed, but it still exists",
                    account
                );
            } else {
                assert!(
                    acc.is_some(),
                    "Expected account {} to exist, but it was closed",
                    account
                );
            }
        }
    }
}

pub trait Expectation {
    fn expect_success(self);
    fn expect_error(self, expectation: ProgramError);
}

impl Expectation for Result<(), BanksClientError> {
    fn expect_success(self) {
        self.unwrap()
    }

    fn expect_error(self, expected_program_error: ProgramError) {
        let err = self
            .expect_err("Expected an error, but transaction succeeded")
            .unwrap();

        if let TransactionError::InstructionError(_, result_instr_error) = err {
            let result_program_error: ProgramError = result_instr_error
                .try_into()
                .expect("Failed to convert InstructionError to ProgramError");

            assert_eq!(
                expected_program_error, result_program_error,
                "Unexpected ProgramError"
            );
        } else {
            panic!("Unexpected error type: {:?}", err);
        }
    }
}

pub async fn get_min_rent_for_size(client: &mut BanksClient, s: usize) -> u64 {
    let rent = client.get_rent().await.unwrap();
    rent.minimum_balance(s)
}

// This function is used to find the correct ATA for the maker and taker wallets,
// it returns a tuple of (maker_ata, taker_ata)
pub fn find_user_ata<T, S>(test_state: &TestStateBase<T, S>) -> (Pubkey, Pubkey)
where
    T: EscrowVariant<S>,
    S: TokenVariant,
{
    if test_state.test_arguments.asset_is_native {
        (
            T::get_program_spec().0, // Returing program id as maker ata if optional
            test_state.taker_wallet.native_token_account, // taker ata is never optional
        )
    } else if test_state.token == NATIVE_MINT {
        (
            test_state.maker_wallet.native_token_account,
            test_state.taker_wallet.native_token_account,
        )
    } else {
        (
            test_state.maker_wallet.token_account,
            test_state.taker_wallet.token_account,
        )
    }
}

pub async fn mint_excess_tokens<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
    escrow_ata: &Pubkey,
    excess_amount: u64,
) {
    S::mint_spl_tokens(
        &mut test_state.context,
        &test_state.token,
        escrow_ata,
        &test_state.payer_kp.pubkey(),
        &test_state.payer_kp,
        excess_amount,
    )
    .await;
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

// These following two macros are used to run the tests against both Token2020 and Token2022 tokens
// by defining a type alias (TestState) with type parameters that represent Token2020/Token2022 and
// including the tests in both contexts.
#[macro_export]
macro_rules! run_for_tokens {
    ($(($token_variant:ty, $module_name: ident)),* | $escrow_variant: ty, $tests: item) => {
        $(mod $module_name {
            use super::*;
            type TestState = TestStateBase<$escrow_variant, $token_variant>;
            $tests
          })*
    };
}
