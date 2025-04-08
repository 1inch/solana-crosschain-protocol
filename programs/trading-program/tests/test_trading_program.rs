use solana_program_test::tokio;
use test_context::test_context;

mod utils;
use utils::{create_escrow_via_trading_program, TestStateTrading};

mod test_trading_program {
    use common_tests::helpers::Expectation;

    use super::*;

    type TestState = TestStateTrading;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation_via_trading_program(test_state: &mut TestState) {
        let (escrow_pda, escrow_ata, trading_pda, trading_ata, transaction) =
            create_escrow_via_trading_program(&mut test_state.base).await;
        test_state
            .base
            .client
            .process_transaction(transaction)
            .await
            .expect_success();
    }
}
