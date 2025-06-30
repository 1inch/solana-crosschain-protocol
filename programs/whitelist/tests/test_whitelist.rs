use crate::helpers::{
    deregister, init_whitelist, register, register_deregister_data, set_authority,
    set_authority_data, TestState,
};
use anchor_lang::{prelude::ProgramError, AccountDeserialize, InstructionData, Space};
use common::constants::DISCRIMINATOR_BYTES;
use common_tests::helpers::*;
use common_tests::whitelist::get_whitelist_access_address;
use solana_program_test::tokio;
use solana_sdk::signer::Signer;

use test_context::test_context;
pub mod helpers;
use whitelist::{self, error::WhitelistError};

mod test_whitelist {

    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_init_whitelist(test_state: &mut TestState) {
        let whitelist_state = init_whitelist(test_state).await;

        let whitelist_data_len = DISCRIMINATOR_BYTES + whitelist::WhitelistState::INIT_SPACE;
        let rent_lamports = get_min_rent_for_size(&mut test_state.client, whitelist_data_len).await;
        assert_eq!(
            rent_lamports,
            test_state
                .client
                .get_balance(whitelist_state)
                .await
                .unwrap()
        );

        let whitelist_state_account = test_state
            .client
            .get_account(whitelist_state)
            .await
            .unwrap()
            .unwrap();
        let whitelist_state: whitelist::WhitelistState =
            whitelist::WhitelistState::try_deserialize(
                &mut whitelist_state_account.data.as_slice(),
            )
            .unwrap();
        assert_eq!(whitelist_state.authority, test_state.authority_kp.pubkey());
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_register_deregister_user(test_state: &mut TestState) {
        init_whitelist(test_state).await;

        let whitelist_access_address = register(test_state).await;

        let whitelist_data_len = DISCRIMINATOR_BYTES + whitelist::ResolverAccess::INIT_SPACE;
        let rent_lamports = get_min_rent_for_size(&mut test_state.client, whitelist_data_len).await;
        assert_eq!(
            rent_lamports,
            test_state
                .client
                .get_balance(whitelist_access_address)
                .await
                .unwrap()
        );

        deregister(test_state).await;

        assert!(test_state
            .client
            .get_account(whitelist_access_address)
            .await
            .unwrap()
            .is_none());
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_register_bump(test_state: &mut TestState) {
        init_whitelist(test_state).await;
        let (_, canonical_bump) = get_whitelist_access_address(&test_state.whitelisted_kp.pubkey());

        let whitelist_access_address = register(test_state).await;

        let whitelist_access_account = test_state
            .client
            .get_account(whitelist_access_address)
            .await
            .unwrap()
            .unwrap();
        let resolver_access: whitelist::ResolverAccess =
            whitelist::ResolverAccess::try_deserialize(
                &mut whitelist_access_account.data.as_slice(),
            )
            .unwrap();
        assert_eq!(resolver_access.bump, canonical_bump);
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_register_twice(test_state: &mut TestState) {
        init_whitelist(test_state).await;

        register(test_state).await;
        // Update the last blockhash to execute the next identical transaction
        test_state.context.last_blockhash = test_state.client.get_latest_blockhash().await.unwrap();
        let instruction_data = InstructionData::data(&whitelist::instruction::Register {
            _user: test_state.whitelisted_kp.pubkey(),
        });
        let (_, tx) = register_deregister_data(test_state, instruction_data);
        test_state
            .client
            .process_transaction(tx)
            .await
            .expect_error(ProgramError::Custom(0));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_set_authority(test_state: &mut TestState) {
        init_whitelist(test_state).await;

        let whitelist_state = set_authority(test_state).await;

        let whitelist_state_account = test_state
            .client
            .get_account(whitelist_state)
            .await
            .unwrap()
            .unwrap();
        let whitelist_state: whitelist::WhitelistState =
            whitelist::WhitelistState::try_deserialize(
                &mut whitelist_state_account.data.as_slice(),
            )
            .unwrap();
        assert_eq!(whitelist_state.authority, test_state.someone_kp.pubkey());
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_new_authority_register_deregister(test_state: &mut TestState) {
        init_whitelist(test_state).await;
        set_authority(test_state).await;

        test_state.authority_kp = test_state.someone_kp.insecure_clone();
        let whitelist_access_address = register(test_state).await;

        let whitelist_data_len = DISCRIMINATOR_BYTES + whitelist::ResolverAccess::INIT_SPACE;
        let rent_lamports = get_min_rent_for_size(&mut test_state.client, whitelist_data_len).await;
        assert_eq!(
            rent_lamports,
            test_state
                .client
                .get_balance(whitelist_access_address)
                .await
                .unwrap()
        );

        deregister(test_state).await;

        assert!(test_state
            .client
            .get_account(whitelist_access_address)
            .await
            .unwrap()
            .is_none());
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_register_wrong_authority(test_state: &mut TestState) {
        init_whitelist(test_state).await;

        let instruction_data = InstructionData::data(&whitelist::instruction::Register {
            _user: test_state.whitelisted_kp.pubkey(),
        });

        test_state.authority_kp = test_state.someone_kp.insecure_clone();
        let (_, tx) = register_deregister_data(test_state, instruction_data);

        test_state
            .client
            .process_transaction(tx)
            .await
            .expect_error(ProgramError::Custom(WhitelistError::Unauthorized.into()));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_deregister_wrong_authority(test_state: &mut TestState) {
        init_whitelist(test_state).await;
        register(test_state).await;

        let instruction_data = InstructionData::data(&whitelist::instruction::Deregister {
            _user: test_state.whitelisted_kp.pubkey(),
        });

        test_state.authority_kp = test_state.someone_kp.insecure_clone();
        let (_, tx) = register_deregister_data(test_state, instruction_data);

        test_state
            .client
            .process_transaction(tx)
            .await
            .expect_error(ProgramError::Custom(WhitelistError::Unauthorized.into()));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_previous_authority_register_deregister(test_state: &mut TestState) {
        init_whitelist(test_state).await;
        set_authority(test_state).await;

        let instruction_data = InstructionData::data(&whitelist::instruction::Register {
            _user: test_state.whitelisted_kp.pubkey(),
        });
        let (_, tx) = register_deregister_data(test_state, instruction_data);

        test_state
            .client
            .process_transaction(tx)
            .await
            .expect_error(ProgramError::Custom(WhitelistError::Unauthorized.into()));

        // Register user and then try to deregister with previous authority
        let previous_kp = test_state.authority_kp.insecure_clone();
        test_state.authority_kp = test_state.someone_kp.insecure_clone();
        register(test_state).await;
        test_state.authority_kp = previous_kp.insecure_clone();

        let instruction_data = InstructionData::data(&whitelist::instruction::Deregister {
            _user: test_state.whitelisted_kp.pubkey(),
        });
        let (_, tx) = register_deregister_data(test_state, instruction_data);

        test_state
            .client
            .process_transaction(tx)
            .await
            .expect_error(ProgramError::Custom(WhitelistError::Unauthorized.into()));
    }

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_set_authority_wrong_authority(test_state: &mut TestState) {
        init_whitelist(test_state).await;

        test_state.authority_kp = test_state.someone_kp.insecure_clone();
        let (_, tx) = set_authority_data(test_state);

        test_state
            .client
            .process_transaction(tx)
            .await
            .expect_error(ProgramError::Custom(WhitelistError::Unauthorized.into()));
    }
}
