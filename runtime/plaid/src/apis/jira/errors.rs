use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub enum JiraError {
    UnexpectedStatusCode(u16),
    NetworkError(reqwest::Error),
    InvalidResponse,
}

impl Display for JiraError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
