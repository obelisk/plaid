use ring::digest;

/// Computes the SHA-256 hash of the input data and returns it as a hexadecimal string.
pub fn sha256_hex(input: &[u8]) -> String {
    let hash = digest::digest(&digest::SHA256, input);
    hex::encode(hash)
}
