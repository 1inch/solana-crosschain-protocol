use anchor_lang::prelude::*;
use solana_program::keccak::hashv;
#[account]
pub struct MerkleProof {
    pub proof: Vec<[u8; 32]>,
    pub index: u64,
    pub hashed_secret: [u8; 32],
}

impl MerkleProof {
    /// Verifies the Merkle proof against the provided Merkle root.
    pub fn verify(&self, root: [u8; 32]) -> bool {
        let leaf = self.hash_leaf();
        let mut computed_hash = leaf;

        for proof_element in &self.proof {
            computed_hash = hashv(&[
                std::cmp::min(proof_element, &computed_hash),
                std::cmp::max(proof_element, &computed_hash),
            ])
            .0;
        }

        computed_hash == root
    }

    /// Computes the hash of the leaf using index and hashed_secret.
    fn hash_leaf(&self) -> [u8; 32] {
        hashv(&[&self.index.to_be_bytes(), &self.hashed_secret[..]]).0
    }
}
