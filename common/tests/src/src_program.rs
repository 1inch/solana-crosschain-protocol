use crate::helpers::*;
use crate::whitelist::get_whitelist_access_address;
use crate::wrap_entry;
use anchor_lang::prelude::AccountInfo;
use anchor_lang::solana_program::hash::hashv;
use anchor_lang::AnchorSerialize;
use anchor_lang::InstructionData;
use solana_program_runtime::invoke_context::BuiltinFunctionWithContext;
use solana_program_test::processor;
use solana_sdk::{signature::Signer, signer::keypair::Keypair, transaction::Transaction};

use anchor_lang::Space;
use anchor_spl::associated_token::{spl_associated_token_account, ID as spl_associated_token_id};

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program::ID as system_program_id,
    sysvar::rent::ID as rent_id,
};

pub struct SrcProgram;

type TestState<S> = TestStateBase<SrcProgram, S>;
impl<S: TokenVariant> EscrowVariant<S> for SrcProgram {
    fn get_program_spec() -> (Pubkey, Option<BuiltinFunctionWithContext>) {
        (
            cross_chain_escrow_src::id(),
            wrap_entry!(cross_chain_escrow_src::entry),
        )
    }

    fn get_withdraw_tx(
        test_state: &TestState<S>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_src::instruction::Withdraw {
                secret: test_state.secret,
            });

        let (_, taker_ata) = find_user_ata(test_state);

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_src::id(),
            accounts: vec![
                AccountMeta::new(test_state.taker_wallet.keypair.pubkey(), true),
                AccountMeta::new_readonly(test_state.token, false),
                AccountMeta::new(*escrow, false),
                AccountMeta::new(*escrow_ata, false),
                AccountMeta::new(taker_ata, false),
                AccountMeta::new_readonly(S::get_token_program_id(), false),
                AccountMeta::new_readonly(system_program_id, false),
            ],
            data: instruction_data,
        };

        Transaction::new_signed_with_payer(
            &[instruction],
            Some(&test_state.payer_kp.pubkey()),
            &[&test_state.context.payer, &test_state.taker_wallet.keypair],
            test_state.context.last_blockhash,
        )
    }

    fn get_public_withdraw_tx(
        test_state: &TestState<S>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
        withdrawer: &Keypair,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_src::instruction::PublicWithdraw {
                secret: test_state.secret,
            });

        let (_, taker_ata) = find_user_ata(test_state);
        let (whitelist_access, _) = get_whitelist_access_address(&withdrawer.pubkey());

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_src::id(),
            accounts: vec![
                AccountMeta::new(test_state.taker_wallet.keypair.pubkey(), false),
                AccountMeta::new(withdrawer.pubkey(), true),
                AccountMeta::new_readonly(whitelist_access, false),
                AccountMeta::new_readonly(test_state.token, false),
                AccountMeta::new(*escrow, false),
                AccountMeta::new(*escrow_ata, false),
                AccountMeta::new(taker_ata, false),
                AccountMeta::new_readonly(S::get_token_program_id(), false),
                AccountMeta::new_readonly(system_program_id, false),
            ],
            data: instruction_data,
        };

        Transaction::new_signed_with_payer(
            &[instruction],
            Some(&test_state.payer_kp.pubkey()), // so that withdrawer does not incurr transaction
            // charges and mess up computation of withdrawer's
            // balance expectation.
            &[&test_state.payer_kp, withdrawer],
            test_state.context.last_blockhash,
        )
    }

    fn get_cancel_tx(
        test_state: &TestState<S>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_src::instruction::CancelEscrow {});

        let (maker_ata, _) = find_user_ata(test_state);

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_src::id(),
            accounts: vec![
                AccountMeta::new(test_state.taker_wallet.keypair.pubkey(), true),
                AccountMeta::new(test_state.maker_wallet.keypair.pubkey(), false),
                AccountMeta::new_readonly(test_state.token, false),
                AccountMeta::new(*escrow, false),
                AccountMeta::new(*escrow_ata, false),
                AccountMeta::new(maker_ata, false),
                AccountMeta::new_readonly(S::get_token_program_id(), false),
                AccountMeta::new_readonly(system_program_id, false),
            ],
            data: instruction_data,
        };

        Transaction::new_signed_with_payer(
            &[instruction],
            Some(&test_state.payer_kp.pubkey()),
            &[&test_state.context.payer, &test_state.taker_wallet.keypair],
            test_state.context.last_blockhash,
        )
    }

    fn get_create_tx(
        test_state: &TestState<S>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_src::instruction::CreateEscrow {
                amount: test_state.test_arguments.escrow_amount,
                dutch_auction_data: test_state.test_arguments.dutch_auction_data.clone(),
                merkle_proof: test_state.test_arguments.merkle_proof.clone(),
            });

        let (order, order_ata) = get_order_addresses(test_state);
        let (whitelist_access, _) =
            get_whitelist_access_address(&test_state.taker_wallet.keypair.pubkey());

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_src::id(),
            accounts: vec![
                AccountMeta::new(test_state.taker_wallet.keypair.pubkey(), true),
                AccountMeta::new_readonly(whitelist_access, false),
                AccountMeta::new(test_state.maker_wallet.keypair.pubkey(), false),
                AccountMeta::new_readonly(test_state.token, false),
                AccountMeta::new(order, false),
                AccountMeta::new(order_ata, false),
                AccountMeta::new(*escrow, false),
                AccountMeta::new(*escrow_ata, false),
                AccountMeta::new_readonly(spl_associated_token_id, false),
                AccountMeta::new_readonly(S::get_token_program_id(), false),
                AccountMeta::new_readonly(system_program_id, false),
            ],
            data: instruction_data,
        };
        Transaction::new_signed_with_payer(
            &[instruction],
            Some(&test_state.payer_kp.pubkey()),
            &[&test_state.context.payer, &test_state.taker_wallet.keypair],
            test_state.context.last_blockhash,
        )
    }

    fn get_rescue_funds_tx(
        test_state: &TestState<S>,
        escrow: &Pubkey,
        token_to_rescue: &Pubkey,
        escrow_ata: &Pubkey,
        taker_ata: &Pubkey,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_src::instruction::RescueFundsForEscrow {
                hashlock: test_state.hashlock.to_bytes(),
                order_hash: test_state.order_hash.to_bytes(),
                maker: test_state.maker_wallet.keypair.pubkey(),
                token: test_state.token,
                amount: test_state.test_arguments.escrow_amount,
                safety_deposit: test_state.test_arguments.safety_deposit,
                rescue_start: test_state.test_arguments.rescue_start,
                rescue_amount: test_state.test_arguments.rescue_amount,
            });

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_src::id(),
            accounts: vec![
                AccountMeta::new(test_state.taker_wallet.keypair.pubkey(), true),
                AccountMeta::new_readonly(*token_to_rescue, false),
                AccountMeta::new(*escrow, false),
                AccountMeta::new(*escrow_ata, false),
                AccountMeta::new(*taker_ata, false),
                AccountMeta::new_readonly(S::get_token_program_id(), false),
                AccountMeta::new_readonly(system_program_id, false),
            ],
            data: instruction_data,
        };

        Transaction::new_signed_with_payer(
            &[instruction],
            Some(&test_state.payer_kp.pubkey()),
            &[&test_state.context.payer, &test_state.taker_wallet.keypair],
            test_state.context.last_blockhash,
        )
    }

    fn get_escrow_data_len() -> usize {
        cross_chain_escrow_src::constants::DISCRIMINATOR_BYTES
            + cross_chain_escrow_src::EscrowSrc::INIT_SPACE
    }
}

pub fn get_order_data_len() -> usize {
    cross_chain_escrow_src::constants::DISCRIMINATOR_BYTES
        + cross_chain_escrow_src::Order::INIT_SPACE
}

pub fn get_order_addresses<S: TokenVariant>(
    test_state: &TestStateBase<SrcProgram, S>,
) -> (Pubkey, Pubkey) {
    let (program_id, _) = <SrcProgram as EscrowVariant<S>>::get_program_spec();
    let (order_pda, _) = Pubkey::find_program_address(
        &[
            b"order",
            test_state.order_hash.as_ref(),
            test_state.hashlock.as_ref(),
            test_state.maker_wallet.keypair.pubkey().as_ref(),
            test_state.token.as_ref(),
            test_state
                .test_arguments
                .order_amount
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
    let order_ata = spl_associated_token_account::get_associated_token_address_with_program_id(
        &order_pda,
        &test_state.token,
        &S::get_token_program_id(),
    );

    (order_pda, order_ata)
}

pub fn create_order_data<S: TokenVariant>(
    test_state: &TestStateBase<SrcProgram, S>,
) -> (Pubkey, Pubkey, Transaction) {
    let (order_pda, order_ata) = get_order_addresses(test_state);
    let transaction: Transaction = get_create_order_tx(test_state, &order_pda, &order_ata);

    (order_pda, order_ata, transaction)
}

pub fn get_create_order_tx<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &TestStateBase<T, S>,
    order: &Pubkey,
    order_ata: &Pubkey,
) -> Transaction {
    let instruction_data = InstructionData::data(&cross_chain_escrow_src::instruction::Create {
        amount: test_state.test_arguments.order_amount,
        parts_amount: test_state.test_arguments.order_parts_amount,
        order_hash: test_state.order_hash.to_bytes(),
        hashlock: test_state.hashlock.to_bytes(),
        safety_deposit: test_state.test_arguments.safety_deposit,
        cancellation_duration: test_state.test_arguments.cancellation_duration,
        finality_duration: test_state.test_arguments.finality_duration,
        public_withdrawal_duration: test_state.test_arguments.public_withdrawal_duration,
        withdrawal_duration: test_state.test_arguments.withdrawal_duration,
        rescue_start: test_state.test_arguments.rescue_start,
        expiration_duration: test_state.test_arguments.expiration_duration,
        asset_is_native: test_state.test_arguments.asset_is_native,
        dst_amount: test_state.test_arguments.dst_amount,
        dutch_auction_data_hash: hashv(&[&test_state
            .test_arguments
            .dutch_auction_data
            .try_to_vec()
            .unwrap()])
        .to_bytes(),
        max_cancellation_premium: test_state.test_arguments.max_cancellation_premium,
        cancellation_auction_duration: test_state.test_arguments.cancellation_auction_duration,
        allow_multiple_fills: test_state.test_arguments.allow_multiple_fills,
        _dst_chain_params: test_state.test_arguments.dst_chain_params.clone(),
    });

    let (maker_ata, _) = find_user_ata(test_state);

    let instruction: Instruction = Instruction {
        program_id: cross_chain_escrow_src::id(),
        accounts: vec![
            AccountMeta::new(test_state.maker_wallet.keypair.pubkey(), true),
            AccountMeta::new_readonly(test_state.token, false),
            AccountMeta::new(maker_ata, false),
            AccountMeta::new(*order, false),
            AccountMeta::new(*order_ata, false),
            AccountMeta::new_readonly(spl_associated_token_id, false),
            AccountMeta::new_readonly(S::get_token_program_id(), false),
            AccountMeta::new_readonly(rent_id, false),
            AccountMeta::new_readonly(system_program_id, false),
        ],
        data: instruction_data,
    };
    Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test_state.payer_kp.pubkey()),
        &[&test_state.context.payer, &test_state.maker_wallet.keypair],
        test_state.context.last_blockhash,
    )
}

pub fn get_cancel_order_tx<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &TestStateBase<T, S>,
    order: &Pubkey,
    order_ata: &Pubkey,
    opt_maker_ata: Option<&Pubkey>,
) -> Transaction {
    let instruction_data =
        InstructionData::data(&cross_chain_escrow_src::instruction::CancelOrder {});

    let maker_ata = if let Some(ata) = opt_maker_ata {
        *ata
    } else {
        let (maker_ata, _) = find_user_ata(test_state);
        maker_ata
    };

    let instruction: Instruction = Instruction {
        program_id: cross_chain_escrow_src::id(),
        accounts: vec![
            AccountMeta::new(test_state.maker_wallet.keypair.pubkey(), true),
            AccountMeta::new_readonly(test_state.token, false),
            AccountMeta::new(*order, false),
            AccountMeta::new(*order_ata, false),
            AccountMeta::new(maker_ata, false),
            AccountMeta::new_readonly(S::get_token_program_id(), false),
            AccountMeta::new_readonly(system_program_id, false),
        ],
        data: instruction_data,
    };

    Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test_state.payer_kp.pubkey()),
        &[&test_state.payer_kp, &test_state.maker_wallet.keypair],
        test_state.context.last_blockhash,
    )
}

pub fn get_cancel_order_by_resolver_tx<T: EscrowVariant<S>, S: TokenVariant>(
    test_state: &TestStateBase<T, S>,
    order: &Pubkey,
    order_ata: &Pubkey,
    opt_maker_ata: Option<&Pubkey>,
) -> Transaction {
    let reward_limit = test_state.test_arguments.reward_limit;
    let instruction_data = InstructionData::data(
        &cross_chain_escrow_src::instruction::CancelOrderByResolver { reward_limit },
    );
    let (whitelist_access, _) =
        get_whitelist_access_address(&test_state.taker_wallet.keypair.pubkey());

    let maker_ata = if let Some(ata) = opt_maker_ata {
        *ata
    } else {
        let (maker_ata, _) = find_user_ata(test_state);
        maker_ata
    };

    let instruction: Instruction = Instruction {
        program_id: cross_chain_escrow_src::id(),
        accounts: vec![
            AccountMeta::new(test_state.taker_wallet.keypair.pubkey(), true),
            AccountMeta::new_readonly(whitelist_access, false),
            AccountMeta::new(test_state.maker_wallet.keypair.pubkey(), false),
            AccountMeta::new_readonly(test_state.token, false),
            AccountMeta::new(*order, false),
            AccountMeta::new(*order_ata, false),
            AccountMeta::new(maker_ata, false),
            AccountMeta::new_readonly(S::get_token_program_id(), false),
            AccountMeta::new_readonly(system_program_id, false),
        ],
        data: instruction_data,
    };

    Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test_state.payer_kp.pubkey()),
        &[&test_state.payer_kp, &test_state.taker_wallet.keypair],
        test_state.context.last_blockhash,
    )
}

pub async fn create_order<S: TokenVariant>(
    test_state: &TestStateBase<SrcProgram, S>,
) -> (Pubkey, Pubkey) {
    let (order_pda, order_ata) = get_order_addresses(test_state);
    let transaction: Transaction = get_create_order_tx(test_state, &order_pda, &order_ata);

    test_state
        .client
        .process_transaction(transaction)
        .await
        .expect_success();

    (order_pda, order_ata)
}
