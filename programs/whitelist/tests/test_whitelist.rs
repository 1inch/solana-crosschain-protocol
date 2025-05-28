use crate::helpers::{
    deregister, get_whitelist_access_address, init_whitelist, register, TestState,
};
use anchor_lang::Space;
use common::constants::DISCRIMINATOR;
use common_tests::helpers::*;
use solana_program_test::tokio;
use solana_sdk::signer::Signer;

use test_context::test_context;
pub mod helpers;

mod test_whitelist {

    use anchor_lang::AccountDeserialize;

    use super::*;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_init_whitelist(test_state: &mut TestState) {
        let whitelist_state = init_whitelist(test_state).await;

        let whitelist_data_len = DISCRIMINATOR + whitelist::WhitelistState::INIT_SPACE;
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

    // Can register and deregister a user from whitelist
    #[test_context(TestState)]
    #[tokio::test]
    async fn test_register_deregister_user(test_state: &mut TestState) {
        init_whitelist(test_state).await;

        let whitelist_access_address = register(test_state).await;

        let whitelist_data_len = DISCRIMINATOR + whitelist::ResolverAccess::INIT_SPACE;
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

    // Stores the canonical bump in the whitelist account
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
}
