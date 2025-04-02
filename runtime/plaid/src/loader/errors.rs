#[derive(Debug)]
pub enum Errors {
    BadFilename,
    ModuleParseFailure,
    ModuleCompilationFailure,
    SshCertsError(sshcerts::error::Error),
    UnauthorizedSigner,
    MissingSignatureFile,
}
