use solana_program::keccak::{hashv, Hash};

/// Hashes a pair of leaves, sorting them before hashing.
pub fn hash_leaf_pairs(left: &Hash, right: &Hash) -> Hash {
    let (first, second) = if left.to_bytes() < right.to_bytes() {
        (left, right)
    } else {
        (right, left)
    };
    hashv(&[first.as_ref(), second.as_ref()])
}

/// Computes the next level of a Merkle tree.
pub fn hash_level(data: &[Hash]) -> Vec<Hash> {
    let mut result = Vec::with_capacity((data.len() + 1) / 2);

    let mut i = 0;
    while i + 1 < data.len() {
        result.push(hash_leaf_pairs(&data[i], &data[i + 1]));
        i += 2;
    }

    // Handle odd length by pairing last element with zero hash
    if data.len() % 2 == 1 {
        result.push(hash_leaf_pairs(&data[data.len() - 1], &Hash::default()));
    }

    result
}

pub fn log2_ceil_bit_magic(mut x: u128) -> u32 {
    if x <= 1 {
        return 0;
    }

    let mut msb = 0;
    let original_x = x;

    if x >= 1 << 64 {
        x >>= 64;
        msb += 64;
    }
    if x >= 1 << 32 {
        x >>= 32;
        msb += 32;
    }
    if x >= 1 << 16 {
        x >>= 16;
        msb += 16;
    }
    if x >= 1 << 8 {
        x >>= 8;
        msb += 8;
    }
    if x >= 1 << 4 {
        x >>= 4;
        msb += 4;
    }
    if x >= 1 << 2 {
        x >>= 2;
        msb += 2;
    }
    if x >= 1 << 1 {
        msb += 1;
    }

    let lsb = original_x & (!original_x + 1);
    if lsb == original_x && msb > 0 {
        msb
    } else {
        msb + 1
    }
}

/// Calculates the Merkle root from a list of leaves.
pub fn get_root(mut data: Vec<Hash>) -> Hash {
    assert!(data.len() > 1, "won't generate root for single leaf");

    while data.len() > 1 {
        data = hash_level(&data);
    }

    data[0]
}

/// Generates a Merkle proof for the node at the given index.
pub fn get_proof(mut data: Vec<Hash>, mut node: usize) -> Vec<Hash> {
    let cap: usize = log2_ceil_bit_magic(data.len() as u128).try_into().unwrap();
    let mut result = Vec::with_capacity(cap);

    while data.len() > 1 {
        let sibling = if node & 1 == 1 {
            // Left sibling
            data[node - 1].clone()
        } else if node + 1 == data.len() {
            // No right sibling, pad with zero
            Hash::default()
        } else {
            // Right sibling
            data[node + 1].clone()
        };
        result.push(sibling);
        node /= 2;
        data = hash_level(&data);
    }
    result
}
