use anchor_lang::{error::ErrorCode, prelude::ProgramError};
use common::error::EscrowError;
use common_tests::helpers::*;
use common_tests::run_for_tokens;
use common_tests::src_program::create_public_escrow_cancel_tx;
use common_tests::src_program::{create_order, SrcProgram};
use common_tests::whitelist::prepare_resolvers;
use solana_program_test::tokio;
use solana_sdk::{signature::Signer, signer::keypair::Keypair};
use test_context::test_context;

pub mod helpers_src;
use helpers_src::*;

use helpers_src::merkle_tree_helpers::{get_proof, get_root};

run_for_tokens!(
    (TokenSPL, token_spl_tests),
    (Token2022, token_2022_tests) | SrcProgram,
    mod token_module {

        use super::*;
        mod test_partial_fill_escrow_creation {

            use super::*;
            use cross_chain_escrow_src::merkle_tree::MerkleProof;
            use solana_sdk::keccak::{hashv, Hash};

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_create_escrow_with_merkle_proof_and_leaf_validation(
                test_state: &mut TestState,
            ) {
                let (order, order_ata) = create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                // Check that the order accounts have not been closed.
                let acc_lookup_result = test_state.client.get_account(order).await.unwrap();
                assert!(acc_lookup_result.is_some());

                let acc_lookup_result = test_state.client.get_account(order_ata).await.unwrap();
                assert!(acc_lookup_result.is_some());
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_create_two_escrows_for_separate_parts(test_state: &mut TestState) {
                let (order, order_ata) = create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;
                test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                // Check that the order accounts have not been closed.
                let acc_lookup_result = test_state.client.get_account(order).await.unwrap();
                assert!(acc_lookup_result.is_some());

                let acc_lookup_result = test_state.client.get_account(order_ata).await.unwrap();
                assert!(acc_lookup_result.is_some());
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_create_escrow_with_merkle_proof_and_leaf_validation_for_full_fill(
                test_state: &mut TestState,
            ) {
                let (order, order_ata) = create_order_for_partial_fill(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_escrow_creation_for_partial_fill(test_state, DEFAULT_ESCROW_AMOUNT).await;

                // Check that the order accounts have been closed.
                let acc_lookup_result = test_state.client.get_account(order).await.unwrap();
                assert!(acc_lookup_result.is_none());

                let acc_lookup_result = test_state.client.get_account(order_ata).await.unwrap();
                assert!(acc_lookup_result.is_none());
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_create_two_escrows_for_full_order(test_state: &mut TestState) {
                let (order, order_ata) = create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;
                test_escrow_creation_for_partial_fill(
                    test_state,
                    DEFAULT_ESCROW_AMOUNT - escrow_amount, // full fill
                )
                .await;

                // Check that the order accounts have been closed.
                let acc_lookup_result = test_state.client.get_account(order).await.unwrap();
                assert!(acc_lookup_result.is_none());

                let acc_lookup_result = test_state.client.get_account(order_ata).await.unwrap();
                assert!(acc_lookup_result.is_none());
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_create_two_escrows_for_full_order_excess_tokens(
                test_state: &mut TestState,
            ) {
                let (order, order_ata) = create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                let excess_amount = 1000;
                // Send excess tokens to the order ATA.
                mint_excess_tokens(test_state, &order_ata, excess_amount).await;
                let (_, escrow_ata) = test_escrow_creation_for_partial_fill(
                    test_state,
                    DEFAULT_ESCROW_AMOUNT - escrow_amount, // full fill
                )
                .await;

                // Check that the escrow ATA was created with the correct amount.
                assert_eq!(
                    test_state.test_arguments.escrow_amount + excess_amount,
                    get_token_balance(&mut test_state.context, &escrow_ata).await
                );

                // Check that the order accounts have been closed.
                let acc_lookup_result = test_state.client.get_account(order).await.unwrap();
                assert!(acc_lookup_result.is_none());

                let acc_lookup_result = test_state.client.get_account(order_ata).await.unwrap();
                assert!(acc_lookup_result.is_none());
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_create_escrow_fails_if_second_escrow_amount_too_large(
                test_state: &mut TestState,
            ) {
                create_order_for_partial_fill(test_state).await;
                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;
                let (_, _, transaction) = test_escrow_creation_for_partial_fill_data(
                    test_state,
                    DEFAULT_ESCROW_AMOUNT - escrow_amount + 1,
                )
                .await;

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(EscrowError::InvalidAmount.into()));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_create_escrow_fails_if_second_escrow_have_same_proof_index(
                test_state: &mut TestState,
            ) {
                create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE + 1;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;
                let so_small_escrow_amount = 1;
                let (_, _, transaction) =
                    test_escrow_creation_for_partial_fill_data(test_state, so_small_escrow_amount)
                        .await;

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(EscrowError::InvalidPartialFill.into()));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_create_escrow_fails_with_incorrect_merkle_root(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.order_parts_amount = DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                let merkle_hashes = compute_merkle_leaves(test_state);
                test_state.hashlock = hashv(&[b"incorrect_root"]);
                test_state.test_arguments.allow_multiple_fills = true;
                create_order(test_state).await;

                test_state.test_arguments.escrow_amount =
                    DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                let index_to_validate = get_index_for_escrow_amount(
                    test_state,
                    test_state.test_arguments.escrow_amount,
                );
                let hashed_secret = merkle_hashes.hashed_secrets[index_to_validate];
                let proof_hashes = get_proof(merkle_hashes.leaves.clone(), index_to_validate);
                let proof = MerkleProof {
                    proof: proof_hashes,
                    index: index_to_validate as u64,
                    hashed_secret,
                };
                test_state.test_arguments.merkle_proof = Some(proof);
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (_, _, transaction) = create_escrow_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(EscrowError::InvalidMerkleProof.into()));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_create_escrow_fails_with_incorrect_secret_for_leaf(
                test_state: &mut TestState,
            ) {
                create_order_for_partial_fill(test_state).await;

                let merkle_hashes = compute_merkle_leaves(test_state);
                test_state.test_arguments.escrow_amount =
                    DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 2;
                let index_to_validate = get_index_for_escrow_amount(
                    test_state,
                    test_state.test_arguments.escrow_amount,
                );
                let proof_hashes = get_proof(merkle_hashes.leaves.clone(), index_to_validate);

                // Incorrect hashed_secret
                let proof = MerkleProof {
                    proof: proof_hashes,
                    index: index_to_validate as u64,
                    hashed_secret: merkle_hashes.hashed_secrets[index_to_validate + 1],
                };
                test_state.test_arguments.merkle_proof = Some(proof);
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (_, _, transaction) = create_escrow_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(EscrowError::InvalidMerkleProof.into()));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_create_escrow_fails_without_merkle_proof(test_state: &mut TestState) {
                create_order_for_partial_fill(test_state).await;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                // test_state.test_arguments.merkle_proof is none
                let (_, _, transaction) = create_escrow_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(
                        EscrowError::InconsistentMerkleProofTrait.into(),
                    ));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_create_escrow_fails_if_multiple_fills_are_false_and_merkle_proof_is_provided(
                test_state: &mut TestState,
            ) {
                test_state.test_arguments.order_parts_amount = DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                let merkle_hashes = compute_merkle_leaves(test_state);
                test_state.test_arguments.order_parts_amount = DEFAULT_PARTS_AMOUNT;
                let root = get_root(merkle_hashes.leaves.clone());
                test_state.hashlock = Hash::new_from_array(root);
                // test_state.test_arguments.allow_multiple_fills is false;
                create_order(test_state).await;

                let index_to_validate = get_index_for_escrow_amount(
                    test_state,
                    test_state.test_arguments.escrow_amount,
                ); // fill the full order
                let hashed_secret = merkle_hashes.hashed_secrets[index_to_validate];
                let proof_hashes = get_proof(merkle_hashes.leaves.clone(), index_to_validate);
                let proof = MerkleProof {
                    proof: proof_hashes,
                    index: index_to_validate as u64,
                    hashed_secret,
                };
                test_state.test_arguments.merkle_proof = Some(proof);
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;
                let (_, _, transaction) = create_escrow_data(test_state);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(
                        EscrowError::InconsistentMerkleProofTrait.into(),
                    ));
            }
        }

        mod test_partial_fill_escrow_withdraw {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_from_partial_order(test_state: &mut TestState) {
                create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                let secret_index = get_index_for_escrow_amount(test_state, escrow_amount);

                let (escrow, escrow_ata) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                test_state.secret = test_state.test_arguments.partial_secrets[secret_index];
                helpers_src::test_withdraw_escrow(test_state, &escrow, &escrow_ata).await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_two_escrows_from_partial_order(test_state: &mut TestState) {
                create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                let secret_index = get_index_for_escrow_amount(test_state, escrow_amount);

                let (escrow, escrow_ata) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                let secret_index_2 = get_index_for_escrow_amount(test_state, escrow_amount);

                let (escrow_2, escrow_ata_2) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                test_state.secret = test_state.test_arguments.partial_secrets[secret_index];
                helpers_src::test_withdraw_escrow(test_state, &escrow, &escrow_ata).await;

                test_state.secret = test_state.test_arguments.partial_secrets[secret_index_2];
                helpers_src::test_withdraw_escrow(test_state, &escrow_2, &escrow_ata_2).await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cannot_withdraw_from_partial_order_with_invalid_secret(
                test_state: &mut TestState,
            ) {
                create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                let (escrow, escrow_ata) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
                );

                test_state.secret = [0u8; 32]; // Invalid secret

                let transaction = SrcProgram::get_withdraw_tx(test_state, &escrow, &escrow_ata);

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(EscrowError::InvalidSecret.into()));
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_withdraw_fails_with_wrong_escrow_pda(test_state: &mut TestState) {
                create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                let (_, escrow_ata) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                let (escrow_2, _) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::Withdrawal as u32,
                );

                let transaction = SrcProgram::get_withdraw_tx(
                    test_state,
                    &escrow_2, // Using wrong escrow PDA
                    &escrow_ata,
                );

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()))
            }
        }

        mod test_partial_fill_escrow_public_withdraw {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_from_partial_order_by_taker(test_state: &mut TestState) {
                create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                let secret_index = get_index_for_escrow_amount(test_state, escrow_amount);

                let (escrow, escrow_ata) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                test_state.secret = test_state.test_arguments.partial_secrets[secret_index];

                let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

                helpers_src::test_public_withdraw_escrow(
                    test_state,
                    &escrow,
                    &escrow_ata,
                    &taker_kp,
                )
                .await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_two_escrows_from_partial_order_by_taker(
                test_state: &mut TestState,
            ) {
                create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                let secret_index = get_index_for_escrow_amount(test_state, escrow_amount);

                let (escrow, escrow_ata) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                let secret_index_2 = get_index_for_escrow_amount(test_state, escrow_amount);

                let (escrow_2, escrow_ata_2) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

                test_state.secret = test_state.test_arguments.partial_secrets[secret_index];

                helpers_src::test_public_withdraw_escrow(
                    test_state,
                    &escrow,
                    &escrow_ata,
                    &taker_kp,
                )
                .await;

                test_state.secret = test_state.test_arguments.partial_secrets[secret_index_2];

                helpers_src::test_public_withdraw_escrow(
                    test_state,
                    &escrow_2,
                    &escrow_ata_2,
                    &taker_kp,
                )
                .await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_from_partial_order_by_any_resolver(
                test_state: &mut TestState,
            ) {
                create_order_for_partial_fill(test_state).await;

                let withdrawer = Keypair::new();

                transfer_lamports(
                    &mut test_state.context,
                    WALLET_DEFAULT_LAMPORTS,
                    &test_state.payer_kp,
                    &withdrawer.pubkey(),
                )
                .await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
                prepare_resolvers(
                    test_state,
                    &[
                        test_state.taker_wallet.keypair.pubkey(),
                        withdrawer.pubkey(),
                    ],
                )
                .await;

                let secret_index = get_index_for_escrow_amount(test_state, escrow_amount);

                let (escrow, escrow_ata) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                test_state.secret = test_state.test_arguments.partial_secrets[secret_index];

                helpers_src::test_public_withdraw_escrow(
                    test_state,
                    &escrow,
                    &escrow_ata,
                    &withdrawer,
                )
                .await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_withdraw_fails_with_wrong_escrow_pda(test_state: &mut TestState) {
                create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                let (_, escrow_ata) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                let (escrow_2, _) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::PublicWithdrawal as u32,
                );

                let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

                let transaction = SrcProgram::get_public_withdraw_tx(
                    test_state,
                    &escrow_2, // Using wrong escrow PDA
                    &escrow_ata,
                    &taker_kp,
                );

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()))
            }
        }

        mod test_partial_fill_escrow_cancel {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_escrow_from_partial_order(test_state: &mut TestState) {
                create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                let (escrow, escrow_ata) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                test_cancel_escrow_partial(test_state, &escrow, &escrow_ata).await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_two_escrows_from_partial_order(test_state: &mut TestState) {
                create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                let (escrow, escrow_ata) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                let (escrow_2, escrow_ata_2) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                test_cancel_escrow_partial(test_state, &escrow, &escrow_ata).await;
                test_cancel_escrow_partial(test_state, &escrow_2, &escrow_ata_2).await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_cancel_fails_with_wrong_escrow_pda(test_state: &mut TestState) {
                create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                let (_, escrow_ata) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                let (escrow_2, _) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::Cancellation as u32,
                );

                let transaction = SrcProgram::get_cancel_tx(
                    test_state,
                    &escrow_2, // Using wrong escrow PDA
                    &escrow_ata,
                );

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()))
            }
        }
        mod test_partial_fill_escrow_public_cancel {
            use super::*;

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_cancel_escrow_from_partial_order_by_taker(
                test_state: &mut TestState,
            ) {
                create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                let (escrow, escrow_ata) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::PublicCancellation as u32,
                );

                let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

                test_public_cancel_escrow(test_state, &escrow, &escrow_ata, &taker_kp).await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_cancel_two_escrows_from_partial_order_by_taker(
                test_state: &mut TestState,
            ) {
                create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                let (escrow, escrow_ata) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                let (escrow_2, escrow_ata_2) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::PublicCancellation as u32,
                );

                let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

                test_public_cancel_escrow(test_state, &escrow, &escrow_ata, &taker_kp).await;
                let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

                test_public_cancel_escrow(test_state, &escrow_2, &escrow_ata_2, &taker_kp).await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_cancel_escrow_from_partial_order_by_any_resolver(
                test_state: &mut TestState,
            ) {
                create_order_for_partial_fill(test_state).await;

                let canceller = Keypair::new();

                transfer_lamports(
                    &mut test_state.context,
                    WALLET_DEFAULT_LAMPORTS,
                    &test_state.payer_kp,
                    &canceller.pubkey(),
                )
                .await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE * 3;
                prepare_resolvers(
                    test_state,
                    &[test_state.taker_wallet.keypair.pubkey(), canceller.pubkey()],
                )
                .await;

                let (escrow, escrow_ata) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::PublicCancellation as u32,
                );

                test_public_cancel_escrow(test_state, &escrow, &escrow_ata, &canceller).await;
            }

            #[test_context(TestState)]
            #[tokio::test]
            async fn test_public_cancel_fails_with_wrong_escrow_pda(test_state: &mut TestState) {
                create_order_for_partial_fill(test_state).await;

                let escrow_amount = DEFAULT_ESCROW_AMOUNT / DEFAULT_PARTS_AMOUNT_FOR_MULTIPLE;
                prepare_resolvers(test_state, &[test_state.taker_wallet.keypair.pubkey()]).await;

                let (_, escrow_ata) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                let (escrow_2, _) =
                    test_escrow_creation_for_partial_fill(test_state, escrow_amount).await;

                set_time(
                    &mut test_state.context,
                    test_state.init_timestamp
                        + DEFAULT_PERIOD_DURATION * PeriodType::PublicCancellation as u32,
                );

                let taker_kp = test_state.taker_wallet.keypair.insecure_clone();

                let transaction = create_public_escrow_cancel_tx(
                    test_state,
                    &escrow_2, // Using wrong escrow PDA
                    &escrow_ata,
                    &taker_kp,
                );

                test_state
                    .client
                    .process_transaction(transaction)
                    .await
                    .expect_error(ProgramError::Custom(ErrorCode::ConstraintTokenOwner.into()))
            }
        }
    }
);
