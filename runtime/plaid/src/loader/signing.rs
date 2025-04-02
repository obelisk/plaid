use super::errors::Errors;
use super::ModuleSigningConfiguration;

use hex::ToHex;
use ring::digest::{digest, SHA256};
use sshcerts::ssh::{SshSignature, VerifiedSshSignature};
use sshcerts::PublicKey;
use std::collections::HashSet;
use std::fs::{self, DirEntry};

/// Checks that a module has enough valid signatures.
///
/// Reads the module’s signature files from its designated subdirectory,
/// verifies each signature against the module's SHA256 hash, and returns
/// `Ok` if the number of valid signatures meets the required threshold.
pub fn check_module_signatures(
    signing: &ModuleSigningConfiguration,
    filename: &str,
    module_bytes: &[u8],
) -> Result<(), Errors> {
    // We expect each module's signatures to be located in a subdirectory of signatures_dir.
    // This subdirectory should share its name with the module
    let module_signatures =
        fs::read_dir(format!("{}/{filename}", signing.signatures_dir))
            .map_err(|e| Errors::FileError(e))?
            .into_iter()
            .filter_map(|entry| {
                entry.inspect_err(|e| {
                    warn!("Bad entry in signature directory for {filename} - skipping. Error: {e}")
                }).ok()
            })
            .collect::<Vec<_>>();

    // If the number of available signature files does not exceed the required count,
    // return an error immediately to avoid unnecessary processing.
    if module_signatures.len() < signing.signatures_required {
        return Err(Errors::NotEnoughValidSignatures(
            0,
            signing.signatures_required,
        ));
    }

    // Hash file
    let module_hash: String = digest(&SHA256, &module_bytes).encode_hex();
    let module_hash = module_hash.as_bytes();

    // We use a HashSet here to ensure that we don't allow the same key to produce multiple valid signatures
    let mut valid_signatures = HashSet::new();
    for signature in module_signatures {
        let pubkey =
            match verify_signature_file(signature, module_hash, &signing.signature_namespace) {
                Ok(pk) => pk,
                Err(e) => {
                    error!("Failed to verify signature for {filename}. Error: {e}");
                    continue;
                }
            };

        // If the provided signature wasn't from an authorized signer, log occurrence
        // and continue processing signature files
        if !signing
            .authorized_signers
            .iter()
            .any(|signer| *signer == pubkey)
        {
            error!(
                "{filename} was signed by an unexpected signer: {}",
                pubkey.fingerprint()
            );
            continue;
        }

        valid_signatures.insert(pubkey.fingerprint().to_string());

        // Return once the threshold is met
        if valid_signatures.len() >= signing.signatures_required {
            return Ok(());
        }
    }

    Err(Errors::NotEnoughValidSignatures(
        valid_signatures.len(),
        signing.signatures_required,
    ))
}

/// Verifies a signature file against a module's hash and ensures it is signed by an authorized signer.
fn verify_signature_file(
    signature_path: DirEntry,
    module_hash: &[u8],
    namespace: &str,
) -> Result<PublicKey, Errors> {
    let signature = read_in_signature(signature_path)?;

    // Parse and validate the signature.
    let ssh_signature =
        SshSignature::from_armored_string(&signature).map_err(|e| Errors::SigningError(e))?;

    let pubkey = ssh_signature.pubkey.clone();

    // Verify the signature using the module hash.
    let verified = VerifiedSshSignature::from_ssh_signature(
        module_hash,
        ssh_signature,
        namespace,
        Some(pubkey),
    )
    .map_err(|e| Errors::SigningError(e))?;

    Ok(verified.signature.pubkey)
}

/// Reads a signature file from the given directory entry.
///
/// Verifies the file ends with ".sig" and returns its content as a String,
/// or an error if the file name is invalid or the file cannot be read.
fn read_in_signature(signature_path: DirEntry) -> Result<String, Errors> {
    let sig_filename = signature_path.file_name().to_string_lossy().to_string();
    if !sig_filename.ends_with(".sig") {
        return Err(Errors::BadFilename(".sig".to_string()));
    }

    // Try to read the signature file.
    std::fs::read_to_string(signature_path.path()).map_err(|e| Errors::FileError(e))
}
