use std::marker::PhantomData;

use common_tests::{
    dst_program::DstProgram,
    helpers::{TestStateBase, TokenVariant},
};

pub fn get_token_account_len<S: TokenVariant>(
    _: PhantomData<TestStateBase<DstProgram, S>>,
) -> usize {
    S::get_token_account_size()
}
