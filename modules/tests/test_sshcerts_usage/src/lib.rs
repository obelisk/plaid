use base64::{prelude::BASE64_STANDARD, Engine};
use plaid_stl::{entrypoint_with_source_and_response, messages::LogSource, plaid};

entrypoint_with_source_and_response!();

const SSH_SK_ED25519_PUBKEY: &str = "sk-ssh-ed25519@openssh.com AAAAGnNrLXNzaC1lZDI1NTE5QG9wZW5zc2guY29tAAAAIEbRzxnNV/AtU+0MwBOLZXwZHou2qCTnMXNMARt231HpAAAABHNzaDo=";

// Test that SSHCerts can be linked and used correctly to verify
// SSH formatted signatures. We use unwraps judiciously because nothing
// should fail here and if it does, then it needs investigation.
fn main(log: String, _source: LogSource) -> Result<Option<String>, i32> {
    plaid::print_debug_string(&format!("Testing sshcerts With Log: [{log}]"));

    // Fetch the signature and data from the web request
    let signature = String::from_utf8(BASE64_STANDARD.decode(plaid::get_query_params("signature").unwrap()).unwrap()).unwrap();
    let data = BASE64_STANDARD.decode(plaid::get_query_params("data").unwrap()).unwrap();

    // Parse the public key and signature
    let public_key = sshcerts::PublicKey::from_string(SSH_SK_ED25519_PUBKEY).unwrap();
    let signature = sshcerts::ssh::SshSignature::from_armored_string(&signature).unwrap();

    // Check the is signed by hardcoded public key
    match sshcerts::ssh::VerifiedSshSignature::from_ssh_signature(&data, signature, "plaid-test", Some(public_key)) {
        Ok(_) => Ok(Some("OK".to_string())),
        Err(e) => Ok(Some(e.to_string()))
    }
}
