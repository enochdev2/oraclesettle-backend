use sha2::{Sha256, Digest};

pub fn hash_leaf(data: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());

    let result = hasher.finalize();

    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}


pub fn build_merkle_root(
    mut leaves: Vec<[u8; 32]>,
) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }

    while leaves.len() > 1 {
        let mut next = Vec::new();

        for pair in leaves.chunks(2) {
            let left = pair[0];
            let right = if pair.len() == 2 {
                pair[1]
            } else {
                pair[0]
            };

            let mut hasher = Sha256::new();

            hasher.update(left);
            hasher.update(right);

            let hash = hasher.finalize();

            let mut out = [0u8; 32];
            out.copy_from_slice(&hash);

            next.push(out);
        }

        leaves = next;
    }

    leaves[0]
}
