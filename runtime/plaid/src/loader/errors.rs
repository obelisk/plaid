use std::fmt::Display;

#[derive(Debug)]
pub enum Errors {
    InvalidFileType(String, String),
    CompileError(wasmer::CompileError),
    SigningError(sshcerts::error::Error),
    NotEnoughValidSignatures(usize, usize),
    FileError(std::io::Error),
}

impl Display for Errors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFileType(expected, received) => write!(
                f,
                "Invalid file type '{received}': expected a {expected} file "
            ),
            Self::CompileError(error) => write!(f, "Wasm compilation error: {error}"),
            Self::SigningError(error) => write!(f, "SshCerts error: {error}"),
            Self::NotEnoughValidSignatures(expected, received) => write!(
                f,
                "Expected {expected} valid signatures but only received {received}"
            ),
            Self::FileError(error) => write!(f, "IO error: {error}"),
        }
    }
}
