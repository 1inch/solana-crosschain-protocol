use solana_program::keccak::hashv;

pub fn hash_leaf_pairs(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let (first, second) = if left < right {
        (left, right)
    } else {
        (right, left)
    };
    hashv(&[first.as_ref(), second.as_ref()]).0
}

pub fn hash_level(data: &[[u8; 32]]) -> Vec<[u8; 32]> {
    let mut result = Vec::with_capacity(data.len().div_ceil(2));

    let mut i = 0;
    while i + 1 < data.len() {
        result.push(hash_leaf_pairs(&data[i], &data[i + 1]));
        i += 2;
    }

    if data.len() % 2 == 1 {
        result.push(hash_leaf_pairs(&data[data.len() - 1], &[0u8; 32]));
    }

    result
}

fn log2_ceil(x: u128) -> u32 {
    if x <= 1 {
        return 0;
    }
    let is_power_of_two = x.is_power_of_two();
    let lz = x.leading_zeros();
    let bits = 128 - lz;
    if is_power_of_two {
        bits - 1
    } else {
        bits
    }
}

pub fn get_root(mut data: Vec<[u8; 32]>) -> [u8; 32] {
    assert!(data.len() > 1, "won't generate root for single leaf");

    while data.len() > 1 {
        data = hash_level(&data);
    }

    data[0]
}

pub fn get_proof(mut data: Vec<[u8; 32]>, mut node: usize) -> Vec<[u8; 32]> {
    let cap: usize = log2_ceil(data.len() as u128).try_into().unwrap();
    let mut result = Vec::with_capacity(cap);

    while data.len() > 1 {
        let sibling = if node & 1 == 1 {
            data[node - 1]
        } else if node + 1 == data.len() {
            [0u8; 32]
        } else {
            data[node + 1]
        };
        result.push(sibling);
        node /= 2;
        data = hash_level(&data);
    }
    result
}
