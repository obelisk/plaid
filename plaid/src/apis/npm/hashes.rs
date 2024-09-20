use ring::digest;

/// Compute the SHA-1 hash of a bytes vector, and return its hex encoding.
pub fn sha1_hex(data: &Vec<u8>) -> String {
    hex::encode(digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, &data).as_ref())
}

/// Compute the SHA-512 hash of a bytes vector, and return its base64 encoding.
pub fn sha512_base64(data: &Vec<u8>) -> String {
    base64::encode(digest::digest(&digest::SHA512, &data).as_ref())
}
