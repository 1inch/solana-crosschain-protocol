use std::any::TypeId;

use crate::{helpers::*, src_program::SrcProgram};
use anchor_lang::error::ErrorCode;
use anchor_spl::token::spl_token::{error::TokenError, native_mint::ID as NATIVE_MINT};
use common::{constants::RESCUE_DELAY, error::EscrowError};
use solana_program::{keccak::hash, program_error::ProgramError};
use solana_sdk::{
    pubkey::Pubkey, signature::Signer, signer::keypair::Keypair, system_instruction::SystemError,
    transaction::Transaction,
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

    let (creator_ata, _) = find_user_ata(test_state);

    // Check token balances for the escrow account and creator are as expected.
    assert_eq!(
        DEFAULT_ESCROW_AMOUNT,
        get_token_balance(&mut test_state.context, &escrow_ata).await
    );
    assert_eq!(
        WALLET_DEFAULT_TOKENS - DEFAULT_ESCROW_AMOUNT,
        get_token_balance(&mut test_state.context, &creator_ata).await
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

pub async fn test_escrow_creation_fail_with_zero_amount<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    test_state.test_arguments.escrow_amount = 0;
    let (_, escrow_ata, transaction) = create_escrow_data(test_state);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((
            0,
            ProgramError::Custom(EscrowError::ZeroAmountOrDeposit.into()),
        ));

    let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
    assert!(acc_lookup_result.is_none());
}

pub async fn test_escrow_creation_fail_with_zero_safety_deposit<
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
        .expect_error((
            0,
            ProgramError::Custom(EscrowError::ZeroAmountOrDeposit.into()),
        ));

    let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
    assert!(acc_lookup_result.is_none());
}

pub async fn test_escrow_creation_fail_with_insufficient_funds<
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
        .expect_error((
            0,
            ProgramError::Custom(EscrowError::SafetyDepositTooLarge.into()),
        ));

    let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
    assert!(acc_lookup_result.is_none());

    let acc_lookup_result = test_state.client.get_account(escrow).await.unwrap();
    assert!(acc_lookup_result.is_none());
}

pub async fn test_escrow_creation_fail_with_insufficient_tokens<
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
        .expect_error((0, ProgramError::from(TokenError::InsufficientFunds)));

    let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
    assert!(acc_lookup_result.is_none());

    let acc_lookup_result = test_state.client.get_account(escrow).await.unwrap();
    assert!(acc_lookup_result.is_none());
}

pub async fn test_escrow_creation_fail_with_existing_order_hash<
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
        transaction.sign(&[&test_state.creator_wallet.keypair], new_hash);
    }
    if transaction.signatures.len() == 2 {
        transaction.sign(
            &[
                &test_state.creator_wallet.keypair,
                &test_state.context.payer,
            ],
            new_hash,
        );
    }
    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((
            0,
            ProgramError::Custom(SystemError::AccountAlreadyInUse as u32),
        ));
}

pub async fn test_escrow_creation_fail_with_invalid_rescue_start<
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
        .expect_error((
            0,
            ProgramError::Custom(EscrowError::InvalidRescueStart.into()),
        ));

    let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
    assert!(acc_lookup_result.is_none());
}

pub async fn test_withdraw<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;
    let transaction = T::get_withdraw_tx(test_state, &escrow, &escrow_ata);

    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;
    let escrow_rent = get_min_rent_for_size(&mut test_state.client, T::get_escrow_data_len()).await;

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
    );

    let (_, recipient_ata) = find_user_ata(test_state);

    test_state
        .expect_balance_change(
            transaction,
            &[
                native_change(
                    test_state.creator_wallet.keypair.pubkey(),
                    token_account_rent + escrow_rent,
                ),
                token_change(recipient_ata, test_state.test_arguments.escrow_amount),
            ],
        )
        .await;

    // Assert escrow was closed
    assert!(test_state
        .client
        .get_account(escrow)
        .await
        .unwrap()
        .is_none());

    // Assert escrow_ata was closed
    assert!(test_state
        .client
        .get_account(escrow_ata)
        .await
        .unwrap()
        .is_none());
}

pub async fn test_withdraw_does_not_work_with_wrong_secret<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    test_state.secret = hash(b"bad-secret").to_bytes();
    let transaction = T::get_withdraw_tx(test_state, &escrow, &escrow_ata);

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::Custom(EscrowError::InvalidSecret.into())));

    // Try to withdraw with zero filled secret.
    test_state.secret = [0u8; 32];
    let transaction = T::get_withdraw_tx(test_state, &escrow, &escrow_ata);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::Custom(EscrowError::InvalidSecret.into())));

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

    test_state.recipient_wallet = test_state.creator_wallet.clone();
    let transaction = T::get_withdraw_tx(test_state, &escrow, &escrow_ata);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::Custom(EscrowError::InvalidAccount.into())))
}

pub async fn test_withdraw_does_not_work_with_wrong_recipient_ata<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    test_state.recipient_wallet.token_account = test_state.creator_wallet.token_account;
    let transaction = T::get_withdraw_tx(test_state, &escrow, &escrow_ata);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((
            0,
            ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()),
        ))
}

pub async fn test_withdraw_does_not_work_with_wrong_escrow_ata<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, _) = create_escrow(test_state).await;

    test_state.test_arguments.escrow_amount += 1;
    let (_, escrow_ata_2) = create_escrow(test_state).await;

    let transaction = T::get_withdraw_tx(test_state, &escrow, &escrow_ata_2);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((
            0,
            ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()),
        ))
}

pub async fn test_withdraw_does_not_work_before_withdrawal_start<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    let transaction = T::get_withdraw_tx(test_state, &escrow, &escrow_ata);

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Finality as u32,
    );
    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::Custom(EscrowError::InvalidTime.into())))
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
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
    );
    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::Custom(EscrowError::InvalidTime.into())))
}

pub async fn test_public_withdraw_tokens<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
    withdrawer: Keypair,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    let transaction = T::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
    );

    // Check that the escrow balance is correct
    assert_eq!(
        get_token_balance(&mut test_state.context, &escrow_ata).await,
        test_state.test_arguments.escrow_amount
    );
    let escrow_data_len = T::get_escrow_data_len();
    let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;
    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;
    assert_eq!(
        rent_lamports,
        test_state.client.get_balance(escrow).await.unwrap()
    );

    let (_, recipient_ata) = find_user_ata(test_state);

    test_state
        .expect_balance_change(
            transaction,
            &[
                native_change(
                    withdrawer.pubkey(),
                    test_state.test_arguments.safety_deposit,
                ),
                native_change(
                    test_state.creator_wallet.keypair.pubkey(),
                    token_account_rent + rent_lamports - test_state.test_arguments.safety_deposit,
                ),
                token_change(recipient_ata, test_state.test_arguments.escrow_amount),
            ],
        )
        .await;

    // Assert accounts were closed
    assert!(test_state
        .client
        .get_account(escrow)
        .await
        .unwrap()
        .is_none());

    // Assert escrow_ata was closed
    assert!(test_state
        .client
        .get_account(escrow_ata)
        .await
        .unwrap()
        .is_none());
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
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::Custom(EscrowError::InvalidSecret.into())))
}

pub async fn test_public_withdraw_fails_with_wrong_recipient_ata<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let withdrawer = test_state.recipient_wallet.keypair.insecure_clone();

    let (escrow, escrow_ata) = create_escrow(test_state).await;

    test_state.recipient_wallet.token_account = test_state.creator_wallet.token_account;
    let transaction = T::get_public_withdraw_tx(test_state, &escrow, &escrow_ata, &withdrawer);

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((
            0,
            ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()),
        ))
}

pub async fn test_public_withdraw_fails_with_wrong_escrow_ata<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let withdrawer = test_state.recipient_wallet.keypair.insecure_clone();

    let (escrow, _) = create_escrow(test_state).await;

    test_state.test_arguments.escrow_amount += 1;
    let (_, escrow_ata_2) = create_escrow(test_state).await;

    let transaction = T::get_public_withdraw_tx(test_state, &escrow, &escrow_ata_2, &withdrawer);

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((
            0,
            ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()),
        ))
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
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::Custom(EscrowError::InvalidTime.into())))
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
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::Custom(EscrowError::InvalidTime.into())))
}

pub async fn test_cancel<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;
    let transaction = T::get_cancel_tx(test_state, &escrow, &escrow_ata);

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
    );

    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;
    let escrow_rent = get_min_rent_for_size(&mut test_state.client, T::get_escrow_data_len()).await;

    let (creator_ata, _) = find_user_ata(test_state);

    test_state
        .expect_balance_change(
            transaction,
            &[
                native_change(
                    test_state.creator_wallet.keypair.pubkey(),
                    escrow_rent + token_account_rent,
                ),
                token_change(creator_ata, test_state.test_arguments.escrow_amount),
            ],
        )
        .await;

    let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
    assert!(acc_lookup_result.is_none());

    let acc_lookup_result = test_state.client.get_account(escrow).await.unwrap();
    assert!(acc_lookup_result.is_none());
}

pub async fn test_cannot_cancel_by_non_creator<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    test_state.creator_wallet = test_state.recipient_wallet.clone();
    let transaction = T::get_cancel_tx(test_state, &escrow, &escrow_ata);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::Custom(EscrowError::InvalidAccount.into())))
}

pub async fn test_cannot_cancel_with_wrong_creator_ata<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    test_state.creator_wallet.token_account = test_state.recipient_wallet.token_account;
    let transaction = T::get_cancel_tx(test_state, &escrow, &escrow_ata);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((
            0,
            ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()),
        ))
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
        .expect_error((
            0,
            ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()),
        ))
}

pub async fn test_cannot_cancel_before_cancellation_start<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;
    let transaction = T::get_cancel_tx(test_state, &escrow, &escrow_ata);

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
    );
    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::Custom(EscrowError::InvalidTime.into())))
}

pub async fn test_escrow_creation_fail_if_finality_duration_overflows<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    test_state.test_arguments.finality_duration = u32::MAX;
    let (_, _, transaction) = create_escrow_data(test_state);
    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::ArithmeticOverflow));
}

pub async fn test_escrow_creation_fail_if_withdrawal_duration_overflows<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    test_state.test_arguments.withdrawal_duration = u32::MAX;
    let (_, _, transaction) = create_escrow_data(test_state);
    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::ArithmeticOverflow));
}

pub async fn test_escrow_creation_fail_if_public_withdrawal_duration_overflows<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    test_state.test_arguments.public_withdrawal_duration = u32::MAX;
    let (_, _, transaction) = create_escrow_data(test_state);
    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::ArithmeticOverflow));
}

pub async fn test_rescue_all_tokens_and_close_ata<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, _) = create_escrow(test_state).await;

    let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
    let escrow_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &escrow)
            .await;
    let recipient_ata = S::initialize_spl_associated_account(
        &mut test_state.context,
        &token_to_rescue,
        &test_state.recipient_wallet.keypair.pubkey(),
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
        &recipient_ata,
    );
    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + RESCUE_DELAY + 100,
    );
    test_state
        .expect_balance_change(
            transaction,
            &[
                native_change(
                    test_state.recipient_wallet.keypair.pubkey(),
                    token_account_rent,
                ),
                token_change(recipient_ata, test_state.test_arguments.rescue_amount),
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

pub async fn test_rescue_part_of_tokens_and_not_close_ata<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, _) = create_escrow(test_state).await;

    let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
    let escrow_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &escrow)
            .await;
    let recipient_ata = S::initialize_spl_associated_account(
        &mut test_state.context,
        &token_to_rescue,
        &test_state.recipient_wallet.keypair.pubkey(),
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

    // Rescue only half of tokens from escrow ata.
    test_state.test_arguments.rescue_amount /= 2;
    let transaction = T::get_rescue_funds_tx(
        test_state,
        &escrow,
        &token_to_rescue,
        &escrow_ata,
        &recipient_ata,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + RESCUE_DELAY + 100,
    );

    test_state
        .expect_balance_change(
            transaction,
            &[token_change(
                recipient_ata,
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
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, _) = create_escrow(test_state).await;

    let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
    let escrow_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &escrow)
            .await;
    let recipient_ata = S::initialize_spl_associated_account(
        &mut test_state.context,
        &token_to_rescue,
        &test_state.recipient_wallet.keypair.pubkey(),
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
        &recipient_ata,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + RESCUE_DELAY - 100,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::Custom(EscrowError::InvalidTime.into())));
}

pub async fn test_cannot_rescue_funds_by_non_recipient<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, _) = create_escrow(test_state).await;

    let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
    let escrow_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &escrow)
            .await;
    test_state.recipient_wallet = test_state.creator_wallet.clone(); // Use different wallet as recipient
    let recipient_ata = S::initialize_spl_associated_account(
        &mut test_state.context,
        &token_to_rescue,
        &test_state.recipient_wallet.keypair.pubkey(),
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
        &recipient_ata,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + RESCUE_DELAY + 100,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::Custom(ErrorCode::ConstraintSeeds.into())))
}

pub async fn test_cannot_rescue_funds_with_wrong_recipient_ata<
    T: EscrowVariant<S>,
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

    let wrong_recipient_ata = S::initialize_spl_associated_account(
        &mut test_state.context,
        &token_to_rescue,
        &test_state.creator_wallet.keypair.pubkey(),
    )
    .await;

    let transaction = T::get_rescue_funds_tx(
        test_state,
        &escrow,
        &token_to_rescue,
        &escrow_ata,
        &wrong_recipient_ata,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + RESCUE_DELAY + 100,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((
            0,
            ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()),
        ))
}

pub async fn test_cannot_rescue_funds_with_wrong_escrow_ata<
    T: EscrowVariant<S>,
    S: TokenVariant,
>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
    let recipient_ata = S::initialize_spl_associated_account(
        &mut test_state.context,
        &token_to_rescue,
        &test_state.recipient_wallet.keypair.pubkey(),
    )
    .await;

    let transaction = T::get_rescue_funds_tx(
        test_state,
        &escrow,
        &token_to_rescue,
        &escrow_ata, // Use escrow ata for escrow mint, but not for token to rescue
        &recipient_ata,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + RESCUE_DELAY + 100,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((
            0,
            ProgramError::Custom(ErrorCode::ConstraintAssociated.into()),
        ))
}

pub async fn test_escrow_creation_native<T: EscrowVariant<S> + 'static, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
    escrow_creator: Pubkey, // The wallet that pays for the escrow creation transaction
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    // Check token balance for the escrow account is as expected.
    assert_eq!(
        DEFAULT_ESCROW_AMOUNT,
        get_token_balance(&mut test_state.context, &escrow_ata).await
    );

    // Check the lamport balance of escrow account is as expected.
    let escrow_data_len = T::get_escrow_data_len();
    let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;
    assert_eq!(
        rent_lamports,
        test_state.client.get_balance(escrow).await.unwrap()
    );

    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;

    let escrow_amount: u64;

    if TypeId::of::<T>() == TypeId::of::<SrcProgram>() {
        escrow_amount = 0; // Expecting the order creator to already have put the tokens in the order account.
    } else {
        escrow_amount = test_state.test_arguments.escrow_amount;
    }

    // Check native balance for the creator is as expected.
    assert_eq!(
        WALLET_DEFAULT_LAMPORTS - escrow_amount - token_account_rent - rent_lamports,
        // The pure lamport balance of the creator wallet after the transaction.
        test_state.client.get_balance(escrow_creator).await.unwrap()
    );
}

pub async fn test_escrow_creation_fail_if_token_is_not_native<
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
        .expect_error((
            0,
            ProgramError::Custom(EscrowError::InconsistentNativeTrait.into()),
        ));
}

pub async fn test_cancel_native<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &mut TestStateBase<T, S>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;
    let transaction = T::get_cancel_tx(test_state, &escrow, &escrow_ata);

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
    );

    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, TokenSPL::get_token_account_size()).await;
    let escrow_rent = get_min_rent_for_size(&mut test_state.client, T::get_escrow_data_len()).await;

    test_state
        .expect_balance_change(
            transaction,
            &[native_change(
                test_state.creator_wallet.keypair.pubkey(),
                escrow_rent + token_account_rent + test_state.test_arguments.escrow_amount,
            )],
        )
        .await;

    let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
    assert!(acc_lookup_result.is_none());

    let acc_lookup_result = test_state.client.get_account(escrow).await.unwrap();
    assert!(acc_lookup_result.is_none());
}
