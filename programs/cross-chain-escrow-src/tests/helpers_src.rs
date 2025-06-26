use std::marker::PhantomData;

use anchor_lang::error::ErrorCode;
use anchor_lang::prelude::ProgramError;
use anchor_spl::token::spl_token::native_mint::ID as NATIVE_MINT;
use common_tests::helpers::{
    account_closure, create_escrow, create_escrow_data, find_user_ata, get_min_rent_for_size,
    get_token_balance, native_change, set_time, token_change, EscrowVariant, Expectation,
    HasTokenVariant, PeriodType, StateChange, TestStateBase, TokenVariant, DEFAULT_ESCROW_AMOUNT,
    DEFAULT_ORDER_SIZE, DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE, DEFAULT_PERIOD_DURATION,
    DEFAULT_SRC_ESCROW_SIZE, WALLET_DEFAULT_LAMPORTS, WALLET_DEFAULT_TOKENS,
};
use common_tests::src_program::{
    create_order, create_order_data, create_public_escrow_cancel_tx,
    get_cancel_order_by_resolver_tx, get_cancel_order_tx, get_order_addresses,
    get_rescue_funds_from_order_tx, SrcProgram,
};
use common_tests::whitelist::prepare_resolvers;
use cross_chain_escrow_src::calculate_premium;
use cross_chain_escrow_src::merkle_tree::MerkleProof;
use primitive_types::U256;
use solana_program::pubkey::Pubkey;
use solana_sdk::clock::Clock;
use solana_sdk::keccak::{hashv, Hash};
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::transaction::Transaction;

use crate::merkle_tree_helpers::{get_proof, get_root};

/// Byte offset in the escrow account data where the `dst_amount` field is located
const DST_AMOUNT_OFFSET: usize = 205;
const U64_SIZE: usize = size_of::<u64>();

/// Reads the `dst_amount` field (u64) directly from the raw account data.
pub fn get_dst_amount(data: &[u8]) -> Option<[u64; 4]> {
    let end = DST_AMOUNT_OFFSET + U64_SIZE * 4;
    let slice = data.get(DST_AMOUNT_OFFSET..end)?;
    Some(U256::from_little_endian(slice).0)
}

pub async fn test_order_creation<S: TokenVariant>(test_state: &mut TestStateBase<SrcProgram, S>) {
    let (order, order_ata, transaction) = create_order_data(test_state);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_success();

    let (maker_ata, _) = find_user_ata(test_state);

    // Check token balance for the order is as expected.
    assert_eq!(
        DEFAULT_ESCROW_AMOUNT,
        get_token_balance(&mut test_state.context, &order_ata).await
    );

    // Check the lamport balance of order account is as expected.
    let order_data_len = DEFAULT_ORDER_SIZE;
    let rent_lamports = get_min_rent_for_size(&mut test_state.client, order_data_len).await;

    let order_ata_lamports =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;
    assert_eq!(
        rent_lamports,
        test_state.client.get_balance(order).await.unwrap()
    );

    // Check the token or lamport balance of creator account is as expected.
    if !test_state.test_arguments.asset_is_native {
        assert_eq!(
            WALLET_DEFAULT_TOKENS - DEFAULT_ESCROW_AMOUNT,
            get_token_balance(&mut test_state.context, &maker_ata).await
        );
    } else {
        // Check native balance for the creator is as expected.
        assert_eq!(
            WALLET_DEFAULT_LAMPORTS - DEFAULT_ESCROW_AMOUNT - order_ata_lamports - rent_lamports,
            // The pure lamport balance of the creator wallet after the transaction.
            test_state
                .client
                .get_balance(test_state.maker_wallet.keypair.pubkey())
                .await
                .unwrap()
        );
    }

    // Calculate the wrapped SOL amount if the token is NATIVE_MINT to adjust the escrow ATA balance.
    let wrapped_sol = if test_state.token == NATIVE_MINT {
        test_state.test_arguments.order_amount
    } else {
        0
    };

    assert_eq!(
        order_ata_lamports,
        test_state.client.get_balance(order_ata).await.unwrap() - wrapped_sol
    );
}

pub async fn test_withdraw_escrow<S: TokenVariant>(test_state: &mut TestStateBase<SrcProgram, S>) {
    create_order(test_state).await;
    prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
    let (escrow, escrow_ata) = create_escrow(test_state).await;
    let transaction = SrcProgram::get_withdraw_tx(test_state, &escrow, &escrow_ata);

    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;

    let escrow_rent = get_min_rent_for_size(&mut test_state.client, DEFAULT_SRC_ESCROW_SIZE).await;

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
    );

    let (_, taker_ata) = find_user_ata(test_state);

    test_state
        .expect_state_change(
            transaction,
            &[
                native_change(
                    test_state.taker_wallet.keypair.pubkey(),
                    token_account_rent + escrow_rent,
                ),
                token_change(taker_ata, test_state.test_arguments.escrow_amount),
                account_closure(escrow, true),
                account_closure(escrow_ata, true),
            ],
        )
        .await;
}

pub async fn test_withdraw_escrow_partial<S: TokenVariant>(
    test_state: &mut TestStateBase<SrcProgram, S>,
    escrow: &Pubkey,
    escrow_ata: &Pubkey,
) {
    let transaction = SrcProgram::get_withdraw_tx(test_state, escrow, escrow_ata);

    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;

    let escrow_rent = get_min_rent_for_size(&mut test_state.client, DEFAULT_SRC_ESCROW_SIZE).await;

    let (_, taker_ata) = find_user_ata(test_state);

    test_state
        .expect_state_change(
            transaction,
            &[
                native_change(
                    test_state.taker_wallet.keypair.pubkey(),
                    token_account_rent + escrow_rent,
                ),
                token_change(taker_ata, test_state.test_arguments.escrow_amount),
                account_closure(*escrow, true),
                account_closure(*escrow_ata, true),
            ],
        )
        .await;
}

pub async fn test_public_withdraw_escrow_partial<S: TokenVariant>(
    test_state: &mut TestStateBase<SrcProgram, S>,
    escrow: &Pubkey,
    escrow_ata: &Pubkey,
    withdrawer: &Keypair,
) {
    let transaction =
        SrcProgram::get_public_withdraw_tx(test_state, escrow, escrow_ata, withdrawer);

    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;

    let escrow_rent = get_min_rent_for_size(&mut test_state.client, DEFAULT_SRC_ESCROW_SIZE).await;

    let (_, taker_ata) = find_user_ata(test_state);

    let balance_changes: Vec<StateChange> = if withdrawer != &test_state.taker_wallet.keypair {
        [
            token_change(taker_ata, test_state.test_arguments.escrow_amount),
            native_change(
                withdrawer.pubkey(),
                test_state.test_arguments.safety_deposit,
            ),
            native_change(
                test_state.taker_wallet.keypair.pubkey(),
                token_account_rent + escrow_rent - test_state.test_arguments.safety_deposit,
            ),
            account_closure(*escrow, true),
            account_closure(*escrow_ata, true),
        ]
        .to_vec()
    } else {
        [
            token_change(taker_ata, test_state.test_arguments.escrow_amount),
            native_change(
                test_state.taker_wallet.keypair.pubkey(),
                token_account_rent + escrow_rent,
            ),
            account_closure(*escrow, true),
            account_closure(*escrow_ata, true),
        ]
        .to_vec()
    };

    test_state
        .expect_state_change(transaction, &balance_changes)
        .await;
}

pub async fn test_cancel_escrow_partial<S: TokenVariant>(
    test_state: &mut TestStateBase<SrcProgram, S>,
    escrow: &Pubkey,
    escrow_ata: &Pubkey,
) {
    let transaction = SrcProgram::get_cancel_tx(test_state, escrow, escrow_ata);

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::PublicCancellation as u32,
    );

    let escrow_data_len = DEFAULT_SRC_ESCROW_SIZE;
    let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;

    test_state
        .expect_state_change(
            transaction,
            &[
                token_change(
                    test_state.maker_wallet.token_account,
                    test_state.test_arguments.escrow_amount,
                ),
                native_change(
                    test_state.taker_wallet.keypair.pubkey(),
                    rent_lamports + token_account_rent,
                ),
                account_closure(*escrow, true),
                account_closure(*escrow_ata, true),
            ],
        )
        .await;
}

pub async fn test_public_cancel_escrow<S: TokenVariant>(
    test_state: &mut TestStateBase<SrcProgram, S>,
    canceller: &Keypair,
) {
    create_order(test_state).await;
    let (escrow, escrow_ata) = create_escrow(test_state).await;

    let transaction = create_public_escrow_cancel_tx(test_state, &escrow, &escrow_ata, canceller);

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + DEFAULT_PERIOD_DURATION * PeriodType::PublicCancellation as u32,
    );

    let escrow_data_len = DEFAULT_SRC_ESCROW_SIZE;
    let rent_lamports = get_min_rent_for_size(&mut test_state.client, escrow_data_len).await;

    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;

    let (maker_ata, _) = find_user_ata(test_state);

    let balance_changes: Vec<StateChange> = if canceller != &test_state.taker_wallet.keypair {
        [
            token_change(maker_ata, DEFAULT_ESCROW_AMOUNT),
            native_change(canceller.pubkey(), test_state.test_arguments.safety_deposit),
            native_change(
                test_state.taker_wallet.keypair.pubkey(),
                rent_lamports + token_account_rent - test_state.test_arguments.safety_deposit,
            ),
            account_closure(escrow, true),
            account_closure(escrow_ata, true),
        ]
        .to_vec()
    } else {
        [
            token_change(maker_ata, DEFAULT_ESCROW_AMOUNT),
            native_change(
                test_state.taker_wallet.keypair.pubkey(),
                rent_lamports + token_account_rent,
            ),
            account_closure(escrow, true),
            account_closure(escrow_ata, true),
        ]
        .to_vec()
    };

    test_state
        .expect_state_change(transaction, &balance_changes)
        .await;
}

pub async fn test_rescue_all_tokens_from_order_and_close_ata<S: TokenVariant>(
    test_state: &mut TestStateBase<SrcProgram, S>,
) {
    let (order, _) = create_order(test_state).await;

    let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
    let order_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &order)
            .await;
    let taker_ata = S::initialize_spl_associated_account(
        &mut test_state.context,
        &token_to_rescue,
        &test_state.taker_wallet.keypair.pubkey(),
    )
    .await;

    S::mint_spl_tokens(
        &mut test_state.context,
        &token_to_rescue,
        &order_ata,
        &test_state.payer_kp.pubkey(),
        &test_state.payer_kp,
        test_state.test_arguments.rescue_amount,
    )
    .await;

    let transaction = get_rescue_funds_from_order_tx(
        test_state,
        &order,
        &order_ata,
        &token_to_rescue,
        &taker_ata,
    );

    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + common::constants::RESCUE_DELAY + 100,
    );
    test_state
        .expect_state_change(
            transaction,
            &[
                native_change(test_state.taker_wallet.keypair.pubkey(), token_account_rent),
                token_change(taker_ata, test_state.test_arguments.rescue_amount),
            ],
        )
        .await;

    // Assert escrow_ata was closed
    assert!(test_state
        .client
        .get_account(order_ata)
        .await
        .unwrap()
        .is_none());
}

pub async fn test_rescue_part_of_tokens_from_order_and_not_close_ata<S: TokenVariant>(
    test_state: &mut TestStateBase<SrcProgram, S>,
) {
    let (order, _) = create_order(test_state).await;

    let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
    let order_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &order)
            .await;
    let taker_ata = S::initialize_spl_associated_account(
        &mut test_state.context,
        &token_to_rescue,
        &test_state.taker_wallet.keypair.pubkey(),
    )
    .await;

    S::mint_spl_tokens(
        &mut test_state.context,
        &token_to_rescue,
        &order_ata,
        &test_state.payer_kp.pubkey(),
        &test_state.payer_kp,
        test_state.test_arguments.rescue_amount,
    )
    .await;

    // Rescue only half of tokens from order ata.
    test_state.test_arguments.rescue_amount /= 2;
    let transaction = get_rescue_funds_from_order_tx(
        test_state,
        &order,
        &order_ata,
        &token_to_rescue,
        &taker_ata,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + common::constants::RESCUE_DELAY + 100,
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

    // Assert order_ata was not closed
    assert!(test_state
        .client
        .get_account(order_ata)
        .await
        .unwrap()
        .is_some());
}

pub async fn _test_cannot_rescue_funds_from_order_by_non_recipient<S: TokenVariant>(
    // TODO: use after implement whitelist
    test_state: &mut TestStateBase<SrcProgram, S>,
) {
    let (order, _) = create_order(test_state).await;

    let token_to_rescue = S::deploy_spl_token(&mut test_state.context).await.pubkey();
    let order_ata =
        S::initialize_spl_associated_account(&mut test_state.context, &token_to_rescue, &order)
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
        &order_ata,
        &test_state.payer_kp.pubkey(),
        &test_state.payer_kp,
        test_state.test_arguments.rescue_amount,
    )
    .await;

    let transaction = get_rescue_funds_from_order_tx(
        test_state,
        &order,
        &order_ata,
        &token_to_rescue,
        &taker_ata,
    );

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + common::constants::RESCUE_DELAY + 100,
    );

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_error(ProgramError::Custom(ErrorCode::ConstraintSeeds.into()))
}

pub async fn test_order_cancel<S: TokenVariant>(test_state: &mut TestStateBase<SrcProgram, S>) {
    let (order, order_ata) = create_order(test_state).await;
    let transaction = get_cancel_order_tx(test_state, &order, &order_ata, None);

    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;
    let order_rent = get_min_rent_for_size(&mut test_state.client, DEFAULT_ORDER_SIZE).await;

    let (maker_ata, _) = find_user_ata(test_state);

    let balance_changes: Vec<StateChange> = if test_state.test_arguments.asset_is_native {
        vec![
            native_change(
                test_state.maker_wallet.keypair.pubkey(),
                token_account_rent + order_rent + test_state.test_arguments.order_amount,
            ),
            account_closure(order, true),
            account_closure(order_ata, true),
        ]
    } else {
        vec![
            token_change(maker_ata, test_state.test_arguments.order_amount),
            native_change(
                test_state.maker_wallet.keypair.pubkey(),
                token_account_rent + order_rent,
            ),
            account_closure(order, true),
            account_closure(order_ata, true),
        ]
    };

    test_state
        .expect_state_change(transaction, &balance_changes)
        .await;
}

pub async fn test_cancel_by_resolver_for_free_at_the_auction_start<S: TokenVariant>(
    test_state: &mut TestStateBase<SrcProgram, S>,
) {
    let (order, order_ata) = create_order(test_state).await;
    let transaction = get_cancel_order_by_resolver_tx(test_state, &order, &order_ata, None);

    set_time(
        &mut test_state.context,
        test_state.init_timestamp + test_state.test_arguments.expiration_duration,
    );

    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;

    let order_rent = get_min_rent_for_size(&mut test_state.client, DEFAULT_ORDER_SIZE).await;

    let (maker_ata, _) = find_user_ata(test_state);

    let balance_changes: Vec<StateChange> = if test_state.test_arguments.asset_is_native {
        vec![
            native_change(
                test_state.maker_wallet.keypair.pubkey(),
                token_account_rent + order_rent + test_state.test_arguments.order_amount,
            ),
            account_closure(order, true),
            account_closure(order_ata, true),
        ]
    } else {
        vec![
            token_change(maker_ata, test_state.test_arguments.order_amount),
            native_change(
                test_state.maker_wallet.keypair.pubkey(),
                token_account_rent + order_rent,
            ),
            account_closure(order, true),
            account_closure(order_ata, true),
        ]
    };

    test_state
        .expect_state_change(transaction, &balance_changes)
        .await;
}

pub async fn test_cancel_by_resolver_at_different_points<S: TokenVariant>(
    init_test_state: &mut TestStateBase<SrcProgram, S>,
    asset_is_native: bool,
    native_mint: Option<Pubkey>,
) {
    let token_account_rent = get_min_rent_for_size(
        &mut init_test_state.client,
        <TestStateBase<SrcProgram, S> as HasTokenVariant>::Token::get_token_account_size(),
    )
    .await;

    let cancellation_points: Vec<u32> = vec![10, 25, 50, 100]
        .into_iter()
        .map(|percentage| {
            (init_test_state.test_arguments.expiration_duration + init_test_state.init_timestamp)
                + (init_test_state.test_arguments.cancellation_auction_duration
                    * (percentage * 100))
                    / (100 * 100)
        })
        .collect();
    prepare_resolvers(
        init_test_state,
        &[init_test_state.taker_wallet.keypair.pubkey()],
    )
    .await;

    for &cancellation_point in &cancellation_points {
        let max_cancellation_premiums: Vec<f64> = vec![1.0, 2.5, 7.5]
            .into_iter()
            .map(|percentage| {
                (token_account_rent as f64 * (percentage * 100_f64)) / (100_f64 * 100_f64)
            })
            .collect();

        for &max_cancellation_premium in &max_cancellation_premiums {
            // Create a new test state for each cancellation point and premium
            let mut test_state =
                reset_test_state(PhantomData::<TestStateBase<SrcProgram, S>>).await;
            prepare_resolvers(&test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

            // Set max cancellation premium
            test_state.test_arguments.max_cancellation_premium = max_cancellation_premium as u64;

            // Ensure reward limit is equal to max cancellation premium
            test_state.test_arguments.reward_limit = max_cancellation_premium as u64;
            if native_mint.is_some() {
                test_state.token = native_mint.unwrap();
            }
            test_state.test_arguments.asset_is_native = asset_is_native;

            let (order, order_ata) = create_order(&test_state).await;
            let transaction =
                get_cancel_order_by_resolver_tx(&test_state, &order, &order_ata, None);

            set_time(&mut test_state.context, cancellation_point);

            let expiratione_time =
                test_state.test_arguments.expiration_duration + test_state.init_timestamp;

            let order_rent =
                get_min_rent_for_size(&mut test_state.client, DEFAULT_ORDER_SIZE).await;

            let clock: Clock = test_state
                .client
                .get_sysvar::<Clock>()
                .await
                .expect("Failed to get Clock sysvar");

            let resolver_premium = calculate_premium(
                clock.unix_timestamp as u32,
                expiratione_time,
                test_state.test_arguments.cancellation_auction_duration,
                max_cancellation_premium as u64,
            );

            let (maker_ata, _) = find_user_ata(&test_state);

            let balance_changes: Vec<StateChange> = if test_state.test_arguments.asset_is_native {
                vec![
                    native_change(
                        test_state.maker_wallet.keypair.pubkey(),
                        token_account_rent + order_rent - resolver_premium
                            + test_state.test_arguments.order_amount,
                    ),
                    native_change(test_state.taker_wallet.keypair.pubkey(), resolver_premium),
                ]
            } else {
                vec![
                    token_change(maker_ata, test_state.test_arguments.order_amount),
                    native_change(
                        test_state.maker_wallet.keypair.pubkey(),
                        token_account_rent + order_rent - resolver_premium,
                    ),
                    native_change(test_state.taker_wallet.keypair.pubkey(), resolver_premium),
                ]
            };

            test_state
                .expect_state_change(transaction, &balance_changes)
                .await;

            let order_acc = test_state.client.get_account(order).await.unwrap();
            assert!(order_acc.is_none());

            let ata_acc = test_state.client.get_account(order_ata).await.unwrap();
            assert!(ata_acc.is_none());
        }
    }
}

pub async fn test_cancel_by_resolver_after_auction<S: TokenVariant>(
    test_state: &mut TestStateBase<SrcProgram, S>,
) {
    let (order, order_ata) = create_order(test_state).await;

    let transaction = get_cancel_order_by_resolver_tx(test_state, &order, &order_ata, None);

    let expiratione_time =
        test_state.test_arguments.expiration_duration + test_state.init_timestamp;

    set_time(
        &mut test_state.context,
        expiratione_time + test_state.test_arguments.cancellation_auction_duration + 1,
    );

    let resolver_premium = test_state.test_arguments.max_cancellation_premium;

    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;

    let order_rent = get_min_rent_for_size(&mut test_state.client, DEFAULT_ORDER_SIZE).await;

    let (maker_ata, _) = find_user_ata(test_state);

    let balance_changes: Vec<StateChange> = if test_state.test_arguments.asset_is_native {
        vec![
            native_change(
                test_state.maker_wallet.keypair.pubkey(),
                token_account_rent + order_rent - resolver_premium
                    + test_state.test_arguments.order_amount,
            ),
            native_change(test_state.taker_wallet.keypair.pubkey(), resolver_premium),
        ]
    } else {
        vec![
            token_change(maker_ata, test_state.test_arguments.order_amount),
            native_change(
                test_state.maker_wallet.keypair.pubkey(),
                token_account_rent + order_rent - resolver_premium,
            ),
            native_change(test_state.taker_wallet.keypair.pubkey(), resolver_premium),
        ]
    };

    test_state
        .expect_state_change(transaction, &balance_changes)
        .await;
}

pub async fn test_cancel_by_resolver_reward_less_then_auction_calculated<S: TokenVariant>(
    test_state: &mut TestStateBase<SrcProgram, S>,
) {
    let (order, order_ata) = create_order(test_state).await;

    let resolver_premium: u64 = 1;

    test_state.test_arguments.reward_limit = resolver_premium;

    let transaction = get_cancel_order_by_resolver_tx(test_state, &order, &order_ata, None);

    let expiratione_time =
        test_state.test_arguments.expiration_duration + test_state.init_timestamp;

    set_time(
        &mut test_state.context,
        expiratione_time + test_state.test_arguments.cancellation_auction_duration + 1,
    );

    let token_account_rent =
        get_min_rent_for_size(&mut test_state.client, S::get_token_account_size()).await;

    let order_rent = get_min_rent_for_size(&mut test_state.client, DEFAULT_ORDER_SIZE).await;

    let (maker_ata, _) = find_user_ata(test_state);

    let balance_changes: Vec<StateChange> = if test_state.test_arguments.asset_is_native {
        vec![
            native_change(
                test_state.maker_wallet.keypair.pubkey(),
                token_account_rent + order_rent - resolver_premium
                    + test_state.test_arguments.order_amount,
            ),
            native_change(test_state.taker_wallet.keypair.pubkey(), resolver_premium),
        ]
    } else {
        vec![
            token_change(maker_ata, test_state.test_arguments.order_amount),
            native_change(
                test_state.maker_wallet.keypair.pubkey(),
                token_account_rent + order_rent - resolver_premium,
            ),
            native_change(test_state.taker_wallet.keypair.pubkey(), resolver_premium),
        ]
    };

    test_state
        .expect_state_change(transaction, &balance_changes)
        .await;
}

pub async fn create_order_for_partial_fill<S: TokenVariant>(
    test_state: &mut TestStateBase<SrcProgram, S>,
) -> (Pubkey, Pubkey) {
    test_state.test_arguments.order_parts_amount = DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
    let merkle_hashes = compute_merkle_leaves(test_state);
    let root = get_root(merkle_hashes.leaves.clone());
    test_state.hashlock = Hash::new_from_array(root);
    test_state.test_arguments.allow_multiple_fills = true;
    create_order(test_state).await
}

pub async fn test_escrow_creation_for_partial_fill_data<S: TokenVariant>(
    test_state: &mut TestStateBase<SrcProgram, S>,
    escrow_amount: u64,
) -> (Pubkey, Pubkey, Transaction) {
    let merkle_hashes = compute_merkle_leaves(test_state);
    let index_to_validate = get_index_for_escrow_amount(test_state, escrow_amount);
    test_state.test_arguments.escrow_amount = escrow_amount;

    let hashed_secret = merkle_hashes.hashed_secrets[index_to_validate];
    let proof_hashes = get_proof(merkle_hashes.leaves.clone(), index_to_validate);
    let proof = MerkleProof {
        proof: proof_hashes,
        index: index_to_validate as u64,
        hashed_secret,
    };

    test_state.test_arguments.merkle_proof = Some(proof);
    test_state.test_arguments.partial_secrets = merkle_hashes.secrets;

    create_escrow_data(test_state)
}

pub async fn test_escrow_creation_for_partial_fill<S: TokenVariant>(
    test_state: &mut TestStateBase<SrcProgram, S>,
    escrow_amount: u64,
) -> (Pubkey, Pubkey) {
    let (escrow, escrow_ata, transaction) =
        test_escrow_creation_for_partial_fill_data(test_state, escrow_amount).await;

    let order_ata = get_order_addresses(test_state).1;

    let expect_amount = get_token_balance(&mut test_state.context, &order_ata).await;

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_success();

    if test_state
        .client
        .get_account(order_ata)
        .await
        .unwrap()
        .is_none()
    {
        assert_eq!(
            expect_amount,
            get_token_balance(&mut test_state.context, &escrow_ata).await
        );
    } else {
        assert_eq!(
            escrow_amount,
            get_token_balance(&mut test_state.context, &escrow_ata).await
        );
    }

    test_state.test_arguments.order_remaining_amount -= escrow_amount;
    (escrow, escrow_ata)
}

pub async fn reset_test_state<T, S: TokenVariant>(
    _: PhantomData<TestStateBase<T, S>>,
) -> TestStateBase<SrcProgram, S> {
    <TestStateBase<SrcProgram, S> as test_context::AsyncTestContext>::setup().await
}

pub struct MerkleHashes {
    pub leaves: Vec<[u8; 32]>,
    pub hashed_secrets: Vec<[u8; 32]>,
    pub secrets: Vec<[u8; 32]>,
}

pub fn compute_merkle_leaves<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &TestStateBase<T, S>,
) -> MerkleHashes {
    let secret_amount = (test_state.test_arguments.order_parts_amount + 1) as usize;
    let mut hashed_leaves = Vec::with_capacity(secret_amount);
    let mut hashed_secrets = Vec::with_capacity(secret_amount);
    let mut secrets = Vec::with_capacity(secret_amount);

    for i in 0..secret_amount {
        let i_bytes = (i as u64).to_be_bytes();
        let secret = hashv(&[&i_bytes]).0; // For example secret is hashv(index)
        secrets.push(secret);
        let hashed_secret = hashv(&[&secret]).0;
        hashed_secrets.push(hashed_secret);

        let pair_data = [&i_bytes[..], &hashed_secret[..]];
        let hashed_pair = hashv(&pair_data).0;
        hashed_leaves.push(hashed_pair);
    }

    MerkleHashes {
        leaves: hashed_leaves,
        hashed_secrets,
        secrets,
    }
}

pub fn get_index_for_escrow_amount<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &TestStateBase<T, S>,
    escrow_amount: u64,
) -> usize {
    if escrow_amount == test_state.test_arguments.order_remaining_amount {
        return test_state.test_arguments.order_parts_amount as usize;
    }
    ((test_state.test_arguments.order_amount - test_state.test_arguments.order_remaining_amount
        + escrow_amount
        - 1)
        * test_state.test_arguments.order_parts_amount
        / test_state.test_arguments.order_amount) as usize
}

pub mod merkle_tree_helpers {
    use solana_sdk::keccak::hashv;

    pub fn hash_nodes(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
        let (first, second) = if left < right {
            (left, right)
        } else {
            (right, left)
        };
        hashv(&[first.as_ref(), second.as_ref()]).0
    }

    pub fn hash_level(data: &[[u8; 32]]) -> Vec<[u8; 32]> {
        let mut result = Vec::with_capacity(data.len().div_ceil(2));

        let mut i = 0;
        while i + 1 < data.len() {
            result.push(hash_nodes(&data[i], &data[i + 1]));
            i += 2;
        }

        if data.len() % 2 == 1 {
            result.push(hash_nodes(&data[data.len() - 1], &[0u8; 32]));
        }

        result
    }

    fn log2_ceil(x: u128) -> u32 {
        if x <= 1 {
            return 0;
        }
        let is_power_of_two = x.is_power_of_two();
        let lz = x.leading_zeros();
        let bits = 128 - lz;
        if is_power_of_two {
            bits - 1
        } else {
            bits
        }
    }

    pub fn get_root(mut data: Vec<[u8; 32]>) -> [u8; 32] {
        assert!(data.len() > 1, "won't generate root for single leaf");

        while data.len() > 1 {
            data = hash_level(&data);
        }

        data[0]
    }

    pub fn get_proof(mut data: Vec<[u8; 32]>, mut node: usize) -> Vec<[u8; 32]> {
        let cap: usize = log2_ceil(data.len() as u128).try_into().unwrap();
        let mut result = Vec::with_capacity(cap);

        while data.len() > 1 {
            let sibling = if node & 1 == 1 {
                data[node - 1]
            } else if node + 1 == data.len() {
                [0u8; 32]
            } else {
                data[node + 1]
            };
            result.push(sibling);
            node /= 2;
            data = hash_level(&data);
        }
        result
    }
}
