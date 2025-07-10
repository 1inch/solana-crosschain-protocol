use std::any::TypeId;

use crate::{
    helpers::*,
    src_program::{get_order_hash, SrcProgram},
};
use anchor_lang::error::ErrorCode;
use anchor_spl::token::spl_token::{error::TokenError, native_mint::ID as NATIVE_MINT};
use common::{constants::RESCUE_DELAY, error::EscrowError, timelocks::Stage};
use solana_program::{keccak::hash, program_error::ProgramError};
use solana_sdk::{
    pubkey::Pubkey, signature::Signer, system_instruction::SystemError, transaction::Transaction,
};

pub async fn test_escrow_creation_tx_cost<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    // NOTE: To actually see the output from this test, use the `--show-output` flag as shown below
    // `cargo test -- --show-output` or
    // `cargo test test_escrow_creation_cost -- --show-output`
    let (_, _, tx) = create_escrow_data(test_state);

    println!(
        "CU cost for create: {}",
        measure_tx_compute_units(test_state, tx).await
    );
}

async fn measure_tx_compute_units<T, S>(
    test_state: &mut TestStateBase<T, S>,
    tx: Transaction,
) -> u64 {
    // Simulate the transaction instead of processing
    let result = test_state
        .client
        .simulate_transaction(tx.clone())
        .await
        .expect("Simulation RPC failed");

    // Extract the simulation details
    let sim_details = result
        .simulation_details
        .expect("Simulation details not found");

    // Return the compute units consumed directly from the simulation
    sim_details.units_consumed
}

pub async fn test_escrow_creation<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    let (maker_ata, _) = find_user_ata(test_state);

    // Check token balances for the escrow account and creator are as expected.
    assert_eq!(
        DEFAULT_ESCROW_AMOUNT,
        get_token_balance(&mut test_state.context, &escrow_ata).await
    );
    assert_eq!(
        WALLET_DEFAULT_TOKENS - DEFAULT_ESCROW_AMOUNT,
        get_token_balance(&mut test_state.context, &maker_ata).await
    );

    // Check the lamport balance of escrow account is as expected.
    let escrow_data_len = T::get_escrow_data_len();
    let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;
    let escrow_ata_lamports =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;
    assert_eq!(
        rent_lamports,
        test_state.client.get_balance(escrow).await.unwrap()
    );

    // Calculate the wrapped SOL amount if the token is NATIVE_MINT to adjust the escrow ATA balance.
    let wrapped_sol = if test_state.token == NATIVE_MINT {
        test_state.test_arguments.escrow_amount
    } else {
        0
    };

    assert_eq!(
        escrow_ata_lamports,
        test_state.client.get_balance(escrow_ata).await.unwrap() - wrapped_sol
    );
}

pub async fn test_escrow_creation_fails_with_zero_amount<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    test_state.test_arguments.escrow_amount = 0;
    let (_, escrow_ata, transaction) = create_escrow_data(test_state);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(
            EscrowError::ZeroAmountOrDeposit.into(),
        ));

    let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
    assert!(acc_lookup_result.is_none());
}

pub async fn test_escrow_creation_fails_with_zero_safety_deposit<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    test_state.test_arguments.safety_deposit = 0;
    let (_, escrow_ata, transaction) = create_escrow_data(test_state);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(
            EscrowError::ZeroAmountOrDeposit.into(),
        ));

    let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
    assert!(acc_lookup_result.is_none());
}

pub async fn test_escrow_creation_fails_with_insufficient_funds<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    test_state.test_arguments.safety_deposit = WALLET_DEFAULT_LAMPORTS + 1;
    let (escrow, escrow_ata, transaction) = create_escrow_data(test_state);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(
            EscrowError::SafetyDepositTooLarge.into(),
        ));

    let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
    assert!(acc_lookup_result.is_none());

    let acc_lookup_result = test_state.client.get_account(escrow).await.unwrap();
    assert!(acc_lookup_result.is_none());
}

pub async fn test_escrow_creation_fails_with_insufficient_tokens<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    test_state.test_arguments.escrow_amount = WALLET_DEFAULT_TOKENS + 1;
    let (escrow, escrow_ata, transaction) = create_escrow_data(test_state);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::from(TokenError::InsufficientFunds));

    let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
    assert!(acc_lookup_result.is_none());

    let acc_lookup_result = test_state.client.get_account(escrow).await.unwrap();
    assert!(acc_lookup_result.is_none());
}

pub async fn test_escrow_creation_fails_with_existing_order_hash<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (_, _, mut transaction) = create_escrow_data(test_state);
    // Send the transaction.
    test_state
        .client
        .process_transaction(transaction.clone())
        .await
        .expect_success();
    let new_hash = test_state.context.get_new_latest_blockhash().await.unwrap();
    if transaction.signatures.len() == 1 {
        transaction.sign(&[&test_state.maker_wallet.keypair], new_hash);
    }
    if transaction.signatures.len() == 2 {
        transaction.sign(
            &[&test_state.maker_wallet.keypair, &test_state.context.payer],
            new_hash,
        );
    }
    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(
            SystemError::AccountAlreadyInUse as u32,
        ));
}

pub async fn test_escrow_creation_fails_with_invalid_rescue_start<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    test_state.test_arguments.rescue_start =
        test_state.test_arguments.init_timestamp + RESCUE_DELAY - 100;
    let (_, escrow_ata, transaction) = create_escrow_data(test_state);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(EscrowError::InvalidRescueStart.into()));

    let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
    assert!(acc_lookup_result.is_none());
}

pub async fn test_withdraw_does_not_work_with_wrong_secret<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    test_state.secret = hash(b"bad-secret").to_bytes();
    let transaction = T::get_withdraw_tx(test_state, &escrow, &escrow_ata);

    set_time(
        &mut test_state.context,
        test_state
            .test_arguments
            .src_timelocks
            .get(Stage::SrcWithdrawal)
            .unwrap(),
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(EscrowError::InvalidSecret.into()));

    // Try to withdraw with zero filled secret.
    test_state.secret = [0u8; 32];
    let transaction = T::get_withdraw_tx(test_state, &escrow, &escrow_ata);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(EscrowError::InvalidSecret.into()));

    assert_eq!(
        get_token_balance(&mut test_state.context, &escrow_ata).await,
        DEFAULT_ESCROW_AMOUNT
    );

    assert_eq!(
        test_state.client.get_balance(escrow).await.unwrap(),
        get_min_rent_for_size(&mut test_state.client, T::get_escrow_data_len()).await
    );
}

pub async fn test_withdraw_does_not_work_with_non_recipient<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    test_state.taker_wallet = test_state.maker_wallet.clone();
    let transaction = T::get_withdraw_tx(test_state, &escrow, &escrow_ata);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(EscrowError::InvalidAccount.into()))
}

pub async fn test_withdraw_does_not_work_with_wrong_taker_ata<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    test_state.taker_wallet.token_account = test_state.maker_wallet.token_account;
    let transaction = T::get_withdraw_tx(test_state, &escrow, &escrow_ata);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()))
}

pub async fn test_withdraw_does_not_work_with_wrong_escrow_ata<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
    new_escrow_amount: u64,
) {
    let (escrow, _) = create_escrow(test_state).await;

    test_state.test_arguments.escrow_amount = new_escrow_amount;
    test_state.test_arguments.order_amount = new_escrow_amount;
    test_state.order_hash = get_order_hash(test_state);
    let (_, escrow_ata_2) = create_escrow(test_state).await;

    let transaction = T::get_withdraw_tx(test_state, &escrow, &escrow_ata_2);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()))
}

pub async fn test_withdraw_does_not_work_before_withdrawal_start<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    let transaction = T::get_withdraw_tx(test_state, &escrow, &escrow_ata);

    set_time(&mut test_state.context, test_state.init_timestamp);
    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(EscrowError::InvalidTime.into()))
}

pub async fn test_withdraw_does_not_work_after_cancellation_start<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    let transaction = T::get_withdraw_tx(test_state, &escrow, &escrow_ata);
    set_time(
        &mut test_state.context,
        test_state
            .test_arguments
            .src_timelocks
            .get(Stage::SrcCancellation)
            .unwrap(),
    );
    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(EscrowError::InvalidTime.into()))
}

pub async fn test_public_withdraw_fails_with_wrong_secret<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let withdrawer = test_state.payer_kp.insecure_clone();
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    test_state.secret = [0u8; 32]; // bad secret
    let transaction = T::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

    set_time(
        &mut test_state.context,
        test_state
            .test_arguments
            .src_timelocks
            .get(Stage::SrcPublicWithdrawal)
            .unwrap(),
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(EscrowError::InvalidSecret.into()))
}

pub async fn test_public_withdraw_fails_with_wrong_taker_ata<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let withdrawer = test_state.taker_wallet.keypair.insecure_clone();

    let (escrow, escrow_ata) = create_escrow(test_state).await;

    test_state.taker_wallet.token_account = test_state.maker_wallet.token_account;
    let transaction = T::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

    set_time(
        &mut test_state.context,
        test_state
            .test_arguments
            .src_timelocks
            .get(Stage::SrcPublicWithdrawal)
            .unwrap(),
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()))
}

pub async fn test_public_withdraw_fails_with_wrong_escrow_ata<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
    new_escrow_amount: u64,
) {
    let withdrawer = test_state.taker_wallet.keypair.insecure_clone();

    let (escrow, _) = create_escrow(test_state).await;

    test_state.test_arguments.escrow_amount = new_escrow_amount;
    test_state.test_arguments.order_amount = new_escrow_amount;
    test_state.order_hash = get_order_hash(test_state);
    let (_, escrow_ata_2) = create_escrow(test_state).await;

    let transaction = T::get_public_withdraw_tx(test_state, &escrow, &escrow_ata_2, &withdrawer);

    set_time(
        &mut test_state.context,
        test_state
            .test_arguments
            .src_timelocks
            .get(Stage::SrcPublicWithdrawal)
            .unwrap(),
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()))
}

pub async fn test_public_withdraw_fails_before_start_of_public_withdraw<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;
    let transaction =
        T::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &test_state.context.payer);

    set_time(
        &mut test_state.context,
        test_state
            .test_arguments
            .src_timelocks
            .get(Stage::SrcWithdrawal)
            .unwrap(),
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(EscrowError::InvalidTime.into()))
}

pub async fn test_public_withdraw_fails_after_cancellation_start<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;
    let transaction =
        T::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &test_state.context.payer);

    set_time(
        &mut test_state.context,
        test_state
            .test_arguments
            .src_timelocks
            .get(Stage::SrcCancellation)
            .unwrap(),
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(EscrowError::InvalidTime.into()))
}

pub async fn test_cancel<T: EscrowVariant<S> + 'static, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
    escrow: &Pubkey,
    escrow_ata: &Pubkey,
) {
    let transaction = T::get_cancel_tx(test_state, escrow, escrow_ata);

    set_time(
        &mut test_state.context,
        test_state
            .test_arguments
            .src_timelocks
            .get(Stage::SrcCancellation)
            .unwrap(),
    );

    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;
    let escrow_rent = get_min_rent_for_size(&mut test_state.client, T::get_escrow_data_len()).await;

    let (maker_ata, _) = find_user_ata(test_state);

    let rent_recipient = if TypeId::of::<T>() == TypeId::of::<SrcProgram>() {
        test_state.taker_wallet.keypair.pubkey()
    } else {
        test_state.maker_wallet.keypair.pubkey()
    };

    test_state
        .expect_state_change(
            transaction,
            &[
                native_change(rent_recipient, escrow_rent + token_account_rent),
                token_change(maker_ata, test_state.test_arguments.escrow_amount),
                account_closure(*escrow_ata, true),
                account_closure(*escrow, true),
            ],
        )
        .await;
}

pub async fn test_cannot_cancel_by_non_maker<T: EscrowVariant<S> + 'static, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;
    if TypeId::of::<T>() == TypeId::of::<SrcProgram>() {
        test_state.taker_wallet = test_state.maker_wallet.clone(); // Use different wallet as recipient
    } else {
        test_state.maker_wallet = test_state.taker_wallet.clone();
    }

    let transaction = T::get_cancel_tx(test_state, &escrow, &escrow_ata);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(EscrowError::InvalidAccount.into()))
}

pub async fn test_cannot_cancel_with_wrong_maker_ata<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    test_state.maker_wallet.token_account = test_state.taker_wallet.token_account;
    let transaction = T::get_cancel_tx(test_state, &escrow, &escrow_ata);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()))
}

pub async fn test_cannot_cancel_with_wrong_escrow_ata<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, _) = create_escrow(test_state).await;

    test_state.test_arguments.escrow_amount += 1;
    let (_, escrow_ata_2) = create_escrow(test_state).await;

    let transaction = T::get_cancel_tx(test_state, &escrow, &escrow_ata_2);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()))
}

pub async fn test_cannot_cancel_before_cancellation_start<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;
    let transaction = T::get_cancel_tx(test_state, &escrow, &escrow_ata);

    set_time(
        &mut test_state.context,
        test_state
            .test_arguments
            .src_timelocks
            .get(Stage::SrcWithdrawal)
            .unwrap(),
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(EscrowError::InvalidTime.into()))
}

pub async fn test_rescue_all_tokens_and_close_ata<
    T: EscrowVariant<S> + 'static,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, _) = create_escrow(test_state).await;

    let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
    let escrow_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &escrow)
            .await;

    S::mint_spl_tokens(
        &mut test_state.context,
        &token_to_rescue,
        &escrow_ata,
        &test_state.payer_kp.pubkey(),
        &test_state.payer_kp,
        test_state.test_arguments.rescue_amount,
    )
    .await;

    let wallet = if TypeId::of::<T>() == TypeId::of::<SrcProgram>() {
        test_state.taker_wallet.keypair.pubkey()
    } else {
        test_state.maker_wallet.keypair.pubkey()
    };

    let taker_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &wallet)
            .await;

    let transaction = T::get_rescue_funds_tx(
        test_state,
        &escrow,
        &token_to_rescue,
        &escrow_ata,
        &taker_ata,
    );
    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + RESCUE_DELAY + 100,
    );
    test_state
        .expect_state_change(
            transaction,
            &[
                native_change(wallet, token_account_rent),
                token_change(taker_ata, test_state.test_arguments.rescue_amount),
            ],
        )
        .await;

    // Assert escrow_ata was closed
    assert!(test_state
        .client
        .get_account(escrow_ata)
        .await
        .unwrap()
        .is_none());
}

pub async fn test_rescue_part_of_tokens_and_not_close_ata<
    T: EscrowVariant<S> + 'static,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, _) = create_escrow(test_state).await;

    let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
    let escrow_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &escrow)
            .await;

    S::mint_spl_tokens(
        &mut test_state.context,
        &token_to_rescue,
        &escrow_ata,
        &test_state.payer_kp.pubkey(),
        &test_state.payer_kp,
        test_state.test_arguments.rescue_amount,
    )
    .await;

    let wallet = if TypeId::of::<T>() == TypeId::of::<SrcProgram>() {
        test_state.taker_wallet.keypair.pubkey()
    } else {
        test_state.maker_wallet.keypair.pubkey()
    };

    let taker_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &wallet)
            .await;

    // Rescue only half of tokens from escrow ata.
    test_state.test_arguments.rescue_amount /= 2;
    let transaction = T::get_rescue_funds_tx(
        test_state,
        &escrow,
        &token_to_rescue,
        &escrow_ata,
        &taker_ata,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + RESCUE_DELAY + 100,
    );

    test_state
        .expect_state_change(
            transaction,
            &[token_change(
                taker_ata,
                test_state.test_arguments.rescue_amount,
            )],
        )
        .await;

    // Assert escrow_ata was not closed
    assert!(test_state
        .client
        .get_account(escrow_ata)
        .await
        .unwrap()
        .is_some());
}

pub async fn test_cannot_rescue_funds_before_rescue_delay_pass<
    T: EscrowVariant<S> + 'static,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, _) = create_escrow(test_state).await;

    let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
    let escrow_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &escrow)
            .await;

    S::mint_spl_tokens(
        &mut test_state.context,
        &token_to_rescue,
        &escrow_ata,
        &test_state.payer_kp.pubkey(),
        &test_state.payer_kp,
        test_state.test_arguments.rescue_amount,
    )
    .await;

    let wallet = if TypeId::of::<T>() == TypeId::of::<SrcProgram>() {
        test_state.taker_wallet.keypair.pubkey()
    } else {
        test_state.maker_wallet.keypair.pubkey()
    };

    let taker_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &wallet)
            .await;

    let transaction = T::get_rescue_funds_tx(
        test_state,
        &escrow,
        &token_to_rescue,
        &escrow_ata,
        &taker_ata,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + RESCUE_DELAY - 100,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(EscrowError::InvalidTime.into()));
}

pub async fn test_cannot_rescue_funds_by_non_recipient<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, _) = create_escrow(test_state).await;

    let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
    let escrow_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &escrow)
            .await;
    test_state.taker_wallet = test_state.maker_wallet.clone(); // Use different wallet as recipient
    let taker_ata = S::initialize_spl_associated_account(
        &mut test_state.context,
        &token_to_rescue,
        &test_state.taker_wallet.keypair.pubkey(),
    )
    .await;

    S::mint_spl_tokens(
        &mut test_state.context,
        &token_to_rescue,
        &escrow_ata,
        &test_state.payer_kp.pubkey(),
        &test_state.payer_kp,
        test_state.test_arguments.rescue_amount,
    )
    .await;

    let transaction = T::get_rescue_funds_tx(
        test_state,
        &escrow,
        &token_to_rescue,
        &escrow_ata,
        &taker_ata,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + RESCUE_DELAY + 100,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(ErrorCode::ConstraintSeeds.into()))
}

pub async fn test_cannot_rescue_funds_with_wrong_taker_ata<
    T: EscrowVariant<S> + 'static,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, _) = create_escrow(test_state).await;

    let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
    let escrow_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &escrow)
            .await;

    S::mint_spl_tokens(
        &mut test_state.context,
        &token_to_rescue,
        &escrow_ata,
        &test_state.payer_kp.pubkey(),
        &test_state.payer_kp,
        test_state.test_arguments.rescue_amount,
    )
    .await;

    let wallet = if TypeId::of::<T>() == TypeId::of::<SrcProgram>() {
        test_state.maker_wallet.keypair.pubkey()
    } else {
        test_state.taker_wallet.keypair.pubkey()
    };

    let wrong_taker_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &wallet)
            .await;

    let transaction = T::get_rescue_funds_tx(
        test_state,
        &escrow,
        &token_to_rescue,
        &escrow_ata,
        &wrong_taker_ata,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + RESCUE_DELAY + 100,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()))
}

pub async fn test_cannot_rescue_funds_with_wrong_escrow_ata<
    T: EscrowVariant<S> + 'static,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();

    let wallet = if TypeId::of::<T>() == TypeId::of::<SrcProgram>() {
        test_state.taker_wallet.keypair.pubkey()
    } else {
        test_state.maker_wallet.keypair.pubkey()
    };

    let taker_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &wallet)
            .await;

    let transaction = T::get_rescue_funds_tx(
        test_state,
        &escrow,
        &token_to_rescue,
        &escrow_ata, // Use escrow ata for escrow mint, but not for token to rescue
        &taker_ata,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + RESCUE_DELAY + 100,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(ErrorCode::ConstraintAssociated.into()))
}

pub async fn test_escrow_creation_fails_if_token_is_not_native<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (_, _, tx) = create_escrow_data(test_state);
    test_state
        .client
        .process_transaction(tx)
        .await
        .expect_error(ProgramError::Custom(
            EscrowError::InconsistentNativeTrait.into(),
        ));
}

#[cfg(test)]
mod test {
    use crate::helpers::*;
    use crate::wrap_entry;
    use common::escrow::{uni_transfer, UniTransferParams};
    use solana_program_test::tokio;
    use solana_sdk::{signature::Signer, transaction::Transaction};

    use anchor_lang::{
        accounts::{interface_account::InterfaceAccount, program::Program},
        prelude::{AccountInfo, AccountMeta, Interface, Pubkey},
    };
    use anchor_spl::token::spl_token::{native_mint::ID as NATIVE_MINT, ID as spl_program_id};
    use solana_program::instruction::Instruction;
    use solana_program_test::{processor, BanksClient, ProgramTest, ProgramTestContext};
    use solana_sdk::{entrypoint::ProgramResult, system_program::ID as system_program_id};

    // Tries to transfer a zero amount via native transfer with non existent target account.
    // Expect to not throw an error since we expect the `uni_transfer` to skip the transaction
    // altogether since the amount is zero.
    #[tokio::test]
    async fn test_uni_transfer_zero_amount_for_native_transfer() {
        let contract_id = Pubkey::new_unique();
        let mut program_test: ProgramTest = ProgramTest::default();
        fn contract<'a>(_: &Pubkey, accounts: &'a [AccountInfo<'a>], _: &[u8]) -> ProgramResult {
            uni_transfer(
                &UniTransferParams::NativeTransfer {
                    from: accounts[1].clone(),
                    to: accounts[2].clone(),
                    amount: 0,
                    program: Program::try_from(&accounts[3]).unwrap(),
                },
                None,
            )?;

            Ok(())
        }
        program_test.add_program("uni-transfer-test", contract_id, wrap_entry!(contract));
        let context: ProgramTestContext = program_test.start_with_context().await;
        let client: BanksClient = context.banks_client.clone();

        let from = Pubkey::new_unique();
        let to = Pubkey::new_unique();
        let instruction: Instruction = Instruction {
            program_id: contract_id,
            accounts: vec![
                AccountMeta::new(context.payer.pubkey(), true),
                AccountMeta::new(from, false),
                AccountMeta::new(to, false),
                AccountMeta::new(system_program_id, false),
            ],
            data: vec![],
        };
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&context.payer.pubkey()),
            &[&context.payer],
            context.last_blockhash,
        );
        client
            .process_transaction(transaction)
            .await
            .expect_success();
    }

    // Same as above, but for token transfers.
    #[tokio::test]
    async fn test_uni_transfer_zero_amount_for_token_transfer() {
        let contract_id = Pubkey::new_unique();
        let mut program_test: ProgramTest = ProgramTest::default();
        fn contract<'a>(_: &Pubkey, accounts: &'a [AccountInfo<'a>], _: &[u8]) -> ProgramResult {
            uni_transfer(
                &UniTransferParams::TokenTransfer {
                    from: accounts[1].clone(),
                    authority: accounts[1].clone(),
                    to: accounts[2].clone(),
                    mint: InterfaceAccount::try_from(&accounts[3]).unwrap(),
                    amount: 0,
                    program: Interface::try_from(&accounts[4]).unwrap(),
                },
                None,
            )?;
            Ok(())
        }
        program_test.add_program("uni-transfer-test", contract_id, wrap_entry!(contract));
        let context: ProgramTestContext = program_test.start_with_context().await;
        let client: BanksClient = context.banks_client.clone();

        let from = Pubkey::new_unique();
        let to = Pubkey::new_unique();
        let instruction: Instruction = Instruction {
            program_id: contract_id,
            accounts: vec![
                AccountMeta::new(context.payer.pubkey(), true),
                AccountMeta::new(from, false),
                AccountMeta::new(to, false),
                AccountMeta::new(NATIVE_MINT, false),
                AccountMeta::new(spl_program_id, false),
            ],
            data: vec![],
        };
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&context.payer.pubkey()),
            &[&context.payer],
            context.last_blockhash,
        );
        client
            .process_transaction(transaction)
            .await
            .expect_success();
    }
}
