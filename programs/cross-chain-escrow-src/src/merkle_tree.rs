pub mod merkle_tree_helpers {
    use solana_program::keccak::hashv;

    pub fn merkle_verify(
        proof: Vec<[u8; 32]>,
        root: [u8; 32],
        index: u32,
        hashed_secret: &[u8; 32],
    ) -> bool {
        let leaf = hash_leaf(index as u64, hashed_secret);
        let mut computed_hash = leaf;
        for proof_element in proof.into_iter() {
            computed_hash = hashv(&[
                std::cmp::min(&proof_element, &computed_hash),
                std::cmp::max(&proof_element, &computed_hash),
            ])
            .0;
        }
        computed_hash == root
    }

    fn hash_leaf(idx: u64, hashed_secret: &[u8; 32]) -> [u8; 32] {
        let i_bytes = idx.to_be_bytes();
        let pair_data = [&i_bytes, &hashed_secret[..]];

        hashv(&pair_data).0
    }
}
