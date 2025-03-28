use crate::helpers::*;
use common::error::EscrowError;
use solana_program::{keccak::hash, program_error::ProgramError};
use solana_sdk::{signature::Signer, transaction::Transaction};

pub async fn test_escrow_creation<T: EscrowVariant>(test_state: &mut TestStateBase<T>) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    // Check token balances for the escrow account and creator are as expected.
    assert_eq!(
        DEFAULT_ESCROW_AMOUNT,
        get_token_balance(&mut test_state.context, &escrow_ata).await
    );
    assert_eq!(
        WALLET_DEFAULT_TOKENS - DEFAULT_ESCROW_AMOUNT,
        get_token_balance(
            &mut test_state.context,
            &test_state.creator_wallet.token_account
        )
        .await
    );

    // Check the lamport balance of escrow account is as expected.
    let escrow_data_len = T::get_escrow_data_len();
    let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;
    assert_eq!(
        rent_lamports,
        test_state.client.get_balance(escrow).await.unwrap()
    );
}

pub async fn test_escrow_creation_fail_with_zero_amount<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    test_state.test_arguments.escrow_amount = 0;
    let (_, escrow_ata, create_ix) = create_escrow_data(test_state);

    let transaction = Transaction::new_signed_with_payer(
        &[create_ix],
        Some(&test_state.payer_kp.pubkey()),
        &[
            &test_state.context.payer,
            &test_state.creator_wallet.keypair,
        ],
        test_state.context.last_blockhash,
    );

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

pub async fn test_escrow_creation_fail_with_zero_safety_deposit<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    test_state.test_arguments.safety_deposit = 0;
    let (_, escrow_ata, create_ix) = create_escrow_data(test_state);

    let transaction = Transaction::new_signed_with_payer(
        &[create_ix],
        Some(&test_state.payer_kp.pubkey()),
        &[
            &test_state.context.payer,
            &test_state.creator_wallet.keypair,
        ],
        test_state.context.last_blockhash,
    );

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

pub async fn test_escrow_creation_fail_with_insufficient_safety_deposit<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    test_state.test_arguments.safety_deposit = WALLET_DEFAULT_LAMPORTS + 1;
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

pub async fn test_escrow_creation_fail_with_insufficient_tokens<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    test_state.test_arguments.escrow_amount = WALLET_DEFAULT_TOKENS + 1;
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

    test_state
        .expect_err_in_tx_meta(transaction, ERROR_INSUFFICIENT_FUNDS)
        .await;

    let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
    assert!(acc_lookup_result.is_none());

    let acc_lookup_result = test_state.client.get_account(escrow).await.unwrap();
    assert!(acc_lookup_result.is_none());
}

pub async fn test_escrow_creation_fail_with_existing_order_hash<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    let (_, _, create_ix) = create_escrow_data(test_state);
    let transaction = Transaction::new_signed_with_payer(
        &[create_ix.clone()],
        Some(&test_state.payer_kp.pubkey()),
        &[
            &test_state.context.payer,
            &test_state.creator_wallet.keypair,
        ],
        test_state.context.last_blockhash,
    );
    // Send the transaction.
    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_success();
    let new_hash = test_state.context.get_new_latest_blockhash().await.unwrap();
    let transaction = Transaction::new_signed_with_payer(
        &[create_ix],
        Some(&test_state.payer_kp.pubkey()),
        &[
            &test_state.context.payer,
            &test_state.creator_wallet.keypair,
        ],
        new_hash, // Use updated last_block_hash so that this
                  // transaction is not rejected silently
                  // for being replayed.
    );
    test_state
        .expect_err_in_tx_meta(transaction, ERROR_ALREADY_USED)
        .await;
}

pub async fn test_withdraw<T: EscrowVariant>(test_state: &mut TestStateBase<T>) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;
    let transaction = T::withdraw_ix_to_signed_tx(
        T::get_withdraw_ix(test_state, &escrow, &escrow_ata),
        test_state,
    );

    let creator_balance_before = test_state
        .client
        .get_balance(test_state.creator_wallet.keypair.pubkey())
        .await
        .unwrap();

    let recipient_token_balance_before = get_token_balance(
        &mut test_state.context,
        &test_state.recipient_wallet.token_account,
    )
    .await;

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_success();

    let creator_balance_after = test_state
        .client
        .get_balance(test_state.creator_wallet.keypair.pubkey())
        .await
        .unwrap();
    let recipient_token_balance_after = get_token_balance(
        &mut test_state.context,
        &test_state.recipient_wallet.token_account,
    )
    .await;

    // Assert lamport for creator is as expected
    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, get_token_account_size()).await;
    let escrow_rent = get_min_rent_for_size(&mut test_state.client, T::get_escrow_data_len()).await;

    assert_eq!(
        creator_balance_before + escrow_rent + token_account_rent,
        creator_balance_after
    );

    // Assert recipient token balance is as expected.
    assert_eq!(
        recipient_token_balance_before + test_state.test_arguments.escrow_amount,
        recipient_token_balance_after,
    );

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

pub async fn test_withdraw_does_not_work_with_wrong_secret<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    test_state.secret = hash(b"bad-secret").to_bytes();
    let transaction = T::withdraw_ix_to_signed_tx(
        T::get_withdraw_ix(test_state, &escrow, &escrow_ata),
        test_state,
    );

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
    let transaction = T::withdraw_ix_to_signed_tx(
        T::get_withdraw_ix(test_state, &escrow, &escrow_ata),
        test_state,
    );

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

pub async fn test_withdraw_does_not_work_with_non_recipient<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    test_state.recipient_wallet = test_state.creator_wallet.clone();
    let withdraw_ix = T::get_withdraw_ix(test_state, &escrow, &escrow_ata);

    let transaction = Transaction::new_signed_with_payer(
        &[withdraw_ix],
        Some(&test_state.payer_kp.pubkey()),
        &[
            &test_state.context.payer,
            &test_state.recipient_wallet.keypair,
        ],
        test_state.context.last_blockhash,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::Custom(EscrowError::InvalidAccount.into())))
}

pub async fn test_withdraw_does_not_work_with_wrong_recipient_ata<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    test_state.recipient_wallet.token_account = test_state.creator_wallet.token_account;
    let transaction = T::withdraw_ix_to_signed_tx(
        T::get_withdraw_ix(test_state, &escrow, &escrow_ata),
        test_state,
    );

    test_state
        .expect_err_in_tx_meta(transaction, ERROR_CONSTRAINT_TOKENOWNER)
        .await;
}

pub async fn test_withdraw_does_not_work_with_wrong_escrow_ata<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    let (escrow, _) = create_escrow(test_state).await;

    test_state.test_arguments.escrow_amount += 1;
    let (_, escrow_ata_2) = create_escrow(test_state).await;

    let withdraw_ix = T::get_withdraw_ix(test_state, &escrow, &escrow_ata_2);

    let transaction = T::withdraw_ix_to_signed_tx(withdraw_ix, test_state);

    test_state
        .expect_err_in_tx_meta(transaction, ERROR_CONSTRAINT_TOKENOWNER)
        .await;
}

pub async fn test_withdraw_does_not_work_before_withdrawal_start<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    let transaction = T::withdraw_ix_to_signed_tx(
        T::get_withdraw_ix(test_state, &escrow, &escrow_ata),
        test_state,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Finality as u32,
    );
    test_state
        .expect_err_in_tx_meta(transaction, ERROR_INVALID_TIME)
        .await;
}

pub async fn test_withdraw_does_not_work_after_cancellation_start<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    let transaction = T::withdraw_ix_to_signed_tx(
        T::get_withdraw_ix(test_state, &escrow, &escrow_ata),
        test_state,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
    );
    test_state
        .expect_err_in_tx_meta(transaction, ERROR_INVALID_TIME)
        .await;
}

pub async fn test_public_withdraw_fails_before_start_of_public_withdraw<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;
    let public_withdraw_ix = T::get_public_withdraw_ix(test_state, &escrow, &escrow_ata);

    let transaction = Transaction::new_signed_with_payer(
        &[public_withdraw_ix],
        Some(&test_state.payer_kp.pubkey()),
        &[&test_state.payer_kp],
        test_state.context.last_blockhash,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
    );

    test_state
        .expect_err_in_tx_meta(transaction, ERROR_INVALID_TIME)
        .await;
}

pub async fn test_public_withdraw_fails_after_cancellation_start<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;
    let public_withdraw_ix = T::get_public_withdraw_ix(test_state, &escrow, &escrow_ata);

    let transaction = Transaction::new_signed_with_payer(
        &[public_withdraw_ix],
        Some(&test_state.payer_kp.pubkey()),
        &[&test_state.payer_kp],
        test_state.context.last_blockhash,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
    );

    test_state
        .expect_err_in_tx_meta(transaction, ERROR_INVALID_TIME)
        .await;
}

pub async fn test_cancel<T: EscrowVariant>(test_state: &mut TestStateBase<T>) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;
    let cancel_ix = T::get_cancel_ix(test_state, &escrow, &escrow_ata);

    let transaction = Transaction::new_signed_with_payer(
        &[cancel_ix],
        Some(&test_state.payer_kp.pubkey()),
        &[
            &test_state.context.payer,
            &test_state.creator_wallet.keypair,
        ],
        test_state.context.last_blockhash,
    );

    let creator_balance_before = test_state
        .client
        .get_balance(test_state.creator_wallet.keypair.pubkey())
        .await
        .unwrap();
    let creator_token_balance_before = get_token_balance(
        &mut test_state.context,
        &test_state.creator_wallet.token_account,
    )
    .await;
    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
    );
    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_success();

    let creator_token_balance_after = get_token_balance(
        &mut test_state.context,
        &test_state.creator_wallet.token_account,
    )
    .await;
    let creator_balance_after = test_state
        .client
        .get_balance(test_state.creator_wallet.keypair.pubkey())
        .await
        .unwrap();

    assert_eq!(
        creator_token_balance_after,
        creator_token_balance_before + test_state.test_arguments.escrow_amount
    );
    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, get_token_account_size()).await;
    let escrow_rent = get_min_rent_for_size(&mut test_state.client, T::get_escrow_data_len()).await;

    assert_eq!(
        creator_balance_before + escrow_rent + token_account_rent,
        creator_balance_after
    );

    let acc_lookup_result = test_state.client.get_account(escrow_ata).await.unwrap();
    assert!(acc_lookup_result.is_none());

    let acc_lookup_result = test_state.client.get_account(escrow).await.unwrap();
    assert!(acc_lookup_result.is_none());
}

pub async fn test_cannot_cancel_by_non_creator<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    test_state.creator_wallet = test_state.recipient_wallet.clone();
    let cancel_ix = T::get_cancel_ix(test_state, &escrow, &escrow_ata);

    let transaction = Transaction::new_signed_with_payer(
        &[cancel_ix],
        Some(&test_state.payer_kp.pubkey()),
        &[
            &test_state.context.payer,
            &test_state.creator_wallet.keypair,
        ],
        test_state.context.last_blockhash,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error((0, ProgramError::Custom(EscrowError::InvalidAccount.into())))
}

pub async fn test_cannot_cancel_with_wrong_creator_ata<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    test_state.creator_wallet.token_account = test_state.recipient_wallet.token_account;
    let cancel_ix = T::get_cancel_ix(test_state, &escrow, &escrow_ata);

    let transaction = Transaction::new_signed_with_payer(
        &[cancel_ix],
        Some(&test_state.payer_kp.pubkey()),
        &[
            &test_state.context.payer,
            &test_state.creator_wallet.keypair,
        ],
        test_state.context.last_blockhash,
    );

    test_state
        .expect_err_in_tx_meta(transaction, ERROR_CONSTRAINT_TOKENOWNER)
        .await;
}

pub async fn test_cannot_cancel_with_wrong_escrow_ata<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    let (escrow, _) = create_escrow(test_state).await;

    test_state.test_arguments.escrow_amount += 1;
    let (_, escrow_ata_2) = create_escrow(test_state).await;

    let cancel_ix = T::get_cancel_ix(test_state, &escrow, &escrow_ata_2);

    let transaction = Transaction::new_signed_with_payer(
        &[cancel_ix],
        Some(&test_state.payer_kp.pubkey()),
        &[
            &test_state.context.payer,
            &test_state.creator_wallet.keypair,
        ],
        test_state.context.last_blockhash,
    );

    test_state
        .expect_err_in_tx_meta(transaction, ERROR_CONSTRAINT_TOKENOWNER)
        .await;
}

pub async fn test_cannot_cancel_before_cancellation_start<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    let (escrow, escrow_ata) = create_escrow(test_state).await;
    let cancel_ix = T::get_cancel_ix(test_state, &escrow, &escrow_ata);

    let transaction = Transaction::new_signed_with_payer(
        &[cancel_ix],
        Some(&test_state.payer_kp.pubkey()),
        &[
            &test_state.context.payer,
            &test_state.creator_wallet.keypair,
        ],
        test_state.context.last_blockhash,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
    );
    test_state
        .expect_err_in_tx_meta(transaction, ERROR_INVALID_TIME)
        .await;
}

pub async fn test_escrow_creation_fail_if_finality_duration_overflows<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    test_state.test_arguments.finality_duration = u32::MAX;
    let (_, _, tx_result) = create_escrow_tx(test_state).await;
    tx_result.expect_error((0, ProgramError::ArithmeticOverflow));
}

pub async fn test_escrow_creation_fail_if_withdrawal_duration_overflows<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    test_state.test_arguments.withdrawal_duration = u32::MAX;
    let (_, _, tx_result) = create_escrow_tx(test_state).await;
    tx_result.expect_error((0, ProgramError::ArithmeticOverflow));
}

pub async fn test_escrow_creation_fail_if_public_withdrawal_duration_overflows<T: EscrowVariant>(
    test_state: &mut TestStateBase<T>,
) {
    test_state.test_arguments.public_withdrawal_duration = u32::MAX;
    let (_, _, tx_result) = create_escrow_tx(test_state).await;
    tx_result.expect_error((0, ProgramError::ArithmeticOverflow));
}
