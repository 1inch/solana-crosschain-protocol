use super::is_valid_partial_fill;

#[test]
fn test_partial_fill_index_does_not_overflow_u64() {
    // 185M tokens with 9 decimals split into 100 parts: on the final fill
    // (filled amount - 1) * parts exceeds u64::MAX.
    let order = 185_000_000 * 1_000_000_000u64;
    let parts = 100;
    let part = order / parts;
    for i in 0..parts - 1 {
        let remaining = order - i * part;
        assert!(is_valid_partial_fill(part, remaining, order, parts, i));
        assert!(!is_valid_partial_fill(part, remaining, order, parts, i + 1));
    }
    // The fill that completes the order must use the extra secret with index `parts`.
    assert!(is_valid_partial_fill(part, part, order, parts, parts));
    assert!(!is_valid_partial_fill(part, part, order, parts, parts - 1));
}

#[test]
fn test_partial_fill_index_with_max_amounts() {
    let max = u64::MAX;
    let parts = u64::from(u16::MAX);
    assert!(is_valid_partial_fill(max, max, max, parts, parts));
    assert!(is_valid_partial_fill(max - 1, max, max, parts, parts - 1));
    assert!(is_valid_partial_fill(1, 1, max, parts, parts));
}
