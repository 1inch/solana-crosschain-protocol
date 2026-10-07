use crate::{helpers::*, whitelist::get_whitelist_access_address};
use solana_program_runtime::invoke_context::BuiltinFunctionWithContext;
use solana_sdk::{signature::Signer, signer::keypair::Keypair, transaction::Transaction};

use anchor_lang::InstructionData;

use anchor_spl::associated_token::{spl_associated_token_account, ID as spl_associated_token_id};

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program::ID as system_program_id,
    sysvar::rent::ID as rent_id,
};

type TestState<S> = TestStateBase<DstProgram, S>;

pub struct DstProgram;

/// Associated token account of `wallet` for the test token
pub fn get_fee_recipient_ata<S: TokenVariant>(
    test_state: &TestState<S>,
    wallet: &Pubkey,
) -> Pubkey {
    spl_associated_token_account::get_associated_token_address_with_program_id(
        wallet,
        &test_state.token,
        &S::get_token_program_id(),
    )
}

/// Account metas of the optional fee recipient accounts of `withdraw` and `public_withdraw`:
/// `[protocol_fee_recipient, protocol_fee_recipient_ata, integrator_fee_recipient, integrator_fee_recipient_ata]`.
/// For a non-zero fee the recipient wallet is passed if the asset is native, its ATA otherwise.
/// The program id stands for a missing (`None`) account.
pub fn get_fee_recipient_metas<S: TokenVariant>(test_state: &TestState<S>) -> Vec<AccountMeta> {
    let fees = &test_state.test_arguments.dst_fee_params;
    let none = AccountMeta::new_readonly(cross_chain_escrow_dst::id(), false);
    let mut metas = Vec::with_capacity(4);
    for (amount, recipient) in [
        (fees.protocol_fee_amount, fees.protocol_fee_recipient),
        (fees.integrator_fee_amount, fees.integrator_fee_recipient),
    ] {
        if amount == 0 {
            metas.push(none.clone());
            metas.push(none.clone());
        } else if test_state.test_arguments.asset_is_native {
            metas.push(AccountMeta::new(recipient, false));
            metas.push(none.clone());
        } else {
            metas.push(none.clone());
            metas.push(AccountMeta::new(
                get_fee_recipient_ata(test_state, &recipient),
                false,
            ));
        }
    }
    metas
}

/// `withdraw` transaction with explicitly given fee recipient account metas, see `get_fee_recipient_metas`
pub fn get_withdraw_tx_with_fee_metas<S: TokenVariant>(
    test_state: &TestState<S>,
    escrow: &Pubkey,
    escrow_ata: &Pubkey,
    fee_recipient_metas: Vec<AccountMeta>,
) -> Transaction {
    let instruction_data = InstructionData::data(&cross_chain_escrow_dst::instruction::Withdraw {
        secret: test_state.secret,
    });

    let (_, taker_ata) = find_user_ata(test_state);

    let mut accounts = vec![
        AccountMeta::new(test_state.maker_wallet.keypair.pubkey(), true),
        AccountMeta::new(test_state.taker_wallet.keypair.pubkey(), false),
        AccountMeta::new_readonly(test_state.token, false),
        AccountMeta::new(*escrow, false),
        AccountMeta::new(*escrow_ata, false),
        AccountMeta::new(taker_ata, false),
    ];
    accounts.extend(fee_recipient_metas);
    accounts.extend([
        AccountMeta::new_readonly(spl_associated_token_id, false),
        AccountMeta::new_readonly(S::get_token_program_id(), false),
        AccountMeta::new_readonly(system_program_id, false),
    ]);

    let instruction: Instruction = Instruction {
        program_id: cross_chain_escrow_dst::id(),
        accounts,
        data: instruction_data,
    };

    Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test_state.payer_kp.pubkey()),
        &[&test_state.context.payer, &test_state.maker_wallet.keypair],
        test_state.context.last_blockhash,
    )
}

impl<S: TokenVariant> EscrowVariant<S> for DstProgram {
    fn sbf_program_name() -> &'static str {
        "cross_chain_escrow_dst"
    }

    fn get_program_spec() -> (Pubkey, Option<BuiltinFunctionWithContext>) {
        (cross_chain_escrow_dst::id(), None)
    }

    fn get_public_withdraw_tx(
        test_state: &TestState<S>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
        withdrawer: &Keypair,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_dst::instruction::PublicWithdraw {
                secret: test_state.secret,
            });

        let (_, taker_ata) = find_user_ata(test_state);
        let (whitelist_access, _) =
            get_whitelist_access_address(&cross_chain_escrow_dst::ID, &withdrawer.pubkey());

        let mut accounts = vec![
            AccountMeta::new(test_state.maker_wallet.keypair.pubkey(), false),
            AccountMeta::new(test_state.taker_wallet.keypair.pubkey(), false),
            AccountMeta::new(withdrawer.pubkey(), true),
            AccountMeta::new_readonly(whitelist_access, false),
            AccountMeta::new_readonly(test_state.token, false),
            AccountMeta::new(*escrow, false),
            AccountMeta::new(*escrow_ata, false),
            AccountMeta::new(taker_ata, false),
        ];
        accounts.extend(get_fee_recipient_metas(test_state));
        accounts.extend([
            AccountMeta::new_readonly(spl_associated_token_id, false),
            AccountMeta::new_readonly(S::get_token_program_id(), false),
            AccountMeta::new_readonly(system_program_id, false),
        ]);

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_dst::id(),
            accounts,
            data: instruction_data,
        };

        Transaction::new_signed_with_payer(
            &[instruction],
            Some(&test_state.payer_kp.pubkey()),
            &[&test_state.payer_kp, withdrawer],
            test_state.context.last_blockhash,
        )
    }

    fn get_withdraw_tx(
        test_state: &TestState<S>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction {
        get_withdraw_tx_with_fee_metas(
            test_state,
            escrow,
            escrow_ata,
            get_fee_recipient_metas(test_state),
        )
    }

    fn get_cancel_tx(
        test_state: &TestState<S>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_dst::instruction::Cancel {});

        let (maker_ata, _) = find_user_ata(test_state);

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_dst::id(),
            accounts: vec![
                AccountMeta::new(test_state.maker_wallet.keypair.pubkey(), true),
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
            &[&test_state.context.payer, &test_state.maker_wallet.keypair],
            test_state.context.last_blockhash,
        )
    }

    fn get_create_tx(
        test_state: &TestState<S>,
        escrow: &Pubkey,
        escrow_ata: &Pubkey,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_dst::instruction::Create {
                amount: test_state.test_arguments.escrow_amount,
                order_hash: test_state.order_hash.to_bytes(),
                hashlock: test_state.hashlock.to_bytes(),
                recipient: test_state.taker_wallet.keypair.pubkey(),
                safety_deposit: test_state.test_arguments.safety_deposit,
                timelocks: test_state.test_arguments.dst_timelocks.get_timelocks(),
                src_cancellation_timestamp: test_state.test_arguments.src_cancellation_timestamp,
                asset_is_native: test_state.test_arguments.asset_is_native,
                fees: test_state.test_arguments.dst_fee_params,
            });

        let (maker_ata, _) = find_user_ata(test_state);

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_dst::id(),
            accounts: vec![
                AccountMeta::new(test_state.maker_wallet.keypair.pubkey(), true),
                AccountMeta::new_readonly(test_state.token, false),
                AccountMeta::new(maker_ata, false),
                AccountMeta::new(*escrow, false),
                AccountMeta::new(*escrow_ata, false),
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

    fn get_rescue_funds_tx(
        test_state: &TestState<S>,
        escrow: &Pubkey,
        token_to_rescue: &Pubkey,
        escrow_ata: &Pubkey,
        taker_ata: &Pubkey,
    ) -> Transaction {
        let instruction_data =
            InstructionData::data(&cross_chain_escrow_dst::instruction::RescueFunds {
                hashlock: test_state.hashlock.to_bytes(),
                order_hash: test_state.order_hash.to_bytes(),
                escrow_amount: test_state.test_arguments.escrow_amount,
                rescue_amount: test_state.test_arguments.rescue_amount,
            });

        let instruction: Instruction = Instruction {
            program_id: cross_chain_escrow_dst::id(),
            accounts: vec![
                AccountMeta::new(test_state.maker_wallet.keypair.pubkey(), true),
                AccountMeta::new_readonly(test_state.taker_wallet.keypair.pubkey(), false),
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
            &[&test_state.context.payer, &test_state.maker_wallet.keypair],
            test_state.context.last_blockhash,
        )
    }

    fn get_escrow_data_len() -> usize {
        DEFAULT_DST_ESCROW_SIZE
    }

    fn get_escrow_creator_wallet(test_state: &TestState<S>) -> Wallet {
        test_state.maker_wallet.clone()
    }
}
