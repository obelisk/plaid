#[derive(Debug)]
#[allow(dead_code)]
pub enum Errors {
    BadFilename,
    ModuleParseFailure,
    ModuleCompilationFailure,
    SigningError(sshcerts::error::Error),
    UnauthorizedSigner,
    NotEnoughValidSignatures,
    FileError(std::io::Error),
}
