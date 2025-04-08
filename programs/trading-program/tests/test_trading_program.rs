use solana_program_test::tokio;
use test_context::test_context;

mod utils;

mod test_escrow_creation_src {
    use super::*;

    type TestState = utils::TestStateTrading;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_via_trading_program(test_state: &mut TestState) {
        let (escrow_pda, escrow_ata, trading_pda, trading_ata, transaction) =
            utils::create_escrow_via_trading_program(&test_state.base);
    }
}
