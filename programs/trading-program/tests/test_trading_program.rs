use common_tests::tests as common_escrow_tests;
use solana_program_test::tokio;
use test_context::test_context;

mod utils;

mod test_escrow_creation_src {
    use super::*;
    use common_tests::src_program::SrcProgram;

    type TestState = utils::TestStateTrading<SrcProgram>;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation(&mut test_state.base).await
    }
}

mod test_escrow_creation_dst {
    use super::*;
    use common_tests::dst_program::DstProgram;

    type TestState = utils::TestStateTrading<DstProgram>;

    #[test_context(TestState)]
    #[tokio::test]
    async fn test_escrow_creation(test_state: &mut TestState) {
        common_escrow_tests::test_escrow_creation(&mut test_state.base).await
    }
}
