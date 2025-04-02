use super::errors::Errors;
use super::ModuleSigningConfiguration;

use hex::ToHex;
use ring::digest::{digest, SHA256};
use sshcerts::ssh::{SshSignature, VerifiedSshSignature};
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
    let module_signatures = fs::read_dir(format!("{}/{filename}", signing.signatures_dir))
        .map_err(|e| Errors::FileError(e))?;

    // Hash file
    let module_hash: String = digest(&SHA256, &module_bytes).encode_hex();
    let module_hash = module_hash.as_bytes();

    // We use a HashSet here to ensure that we don't allow the same key to produce multiple valid signatures
    let mut valid_signatures = HashSet::new();
    for signature in module_signatures {
        let entry = match signature {
            Ok(path) => path,
            Err(e) => {
                error!("Bad entry in signature directory for {filename} - skipping. Error: {e}");
                continue;
            }
        };

        if let Ok(fingerprint) = verify_signature_file(entry, module_hash, &signing, &filename) {
            valid_signatures.insert(fingerprint);
        }

        // Return early once the threshold is met.
        if valid_signatures.len() >= signing.signatures_required {
            return Ok(());
        }
    }

    error!(
        "Not enough valid signatures provided for {filename}. Got {} but needed {}",
        valid_signatures.len(),
        signing.signatures_required
    );
    Err(Errors::NotEnoughValidSignatures)
}

/// Verifies a signature file against a module's hash and ensures it is signed by an authorized signer.
fn verify_signature_file(
    signature_path: DirEntry,
    module_hash: &[u8],
    signing: &ModuleSigningConfiguration,
    module_name: &str,
) -> Result<String, Errors> {
    let signature = read_in_signature(signature_path)?;

    // Parse and validate the signature.
    let ssh_signature = match SshSignature::from_armored_string(&signature) {
        Ok(sig) => sig,
        Err(e) => {
            error!("Invalid signature provided for {}", module_name);
            return Err(Errors::SigningError(e));
        }
    };

    let pubkey = ssh_signature.pubkey.clone();

    // Check if the signature was made by an authorized signer.
    if !signing.authorized_signers.iter().any(|s| *s == pubkey) {
        error!(
            "{} was signed by an unexpected signer: {}",
            module_name,
            pubkey.fingerprint()
        );
        return Err(Errors::UnauthorizedSigner);
    }

    // Verify the signature using the module hash.
    let verified = VerifiedSshSignature::from_ssh_signature(
        module_hash,
        ssh_signature,
        &signing.signature_namespace,
        Some(pubkey),
    )
    .map_err(|e| Errors::SigningError(e))?;

    Ok(verified.signature.pubkey.fingerprint().to_string())
}

/// Reads a signature file from the given directory entry.
///
/// Verifies the file ends with ".sig" and returns its content as a String,
/// or an error if the file name is invalid or the file cannot be read.
fn read_in_signature(signature_path: DirEntry) -> Result<String, Errors> {
    let sig_filename = signature_path.file_name().to_string_lossy().to_string();
    if !sig_filename.ends_with(".sig") {
        return Err(Errors::BadFilename);
    }

    // Try to read the signature file.
    std::fs::read_to_string(signature_path.path()).map_err(|e| {
        error!(
            "Failed to read signature at [{:?}]. Error: {}",
            signature_path, e
        );
        Errors::FileError(e)
    })
}
