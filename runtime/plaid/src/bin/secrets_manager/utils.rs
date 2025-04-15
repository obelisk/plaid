use std::fmt::Display;

/// Take something that looks like `plaid-<deployment>-plaid-<something>` or
/// `plaid-<deployment>-ingress-<something>` and extract `<something>`.
pub fn strip_secret_name(
    instance: impl Display,
    deployment: impl Display,
    s: impl Display,
) -> String {
    let input = s.to_string();
    let stripped = input
        .strip_prefix(&format!("plaid-{deployment}-{instance}-"))
        .unwrap()
        .to_string();
    stripped
}

/// Take something that looks like `secret-name`
/// and turn it into `plaid-<deployment>-plaid-<secret-name>` or `plaid-<deployment>-ingress-<secret-name>`
/// depending on which deployment and instance we are processing for.
///
/// Note - This method will panic if the input string is not in the expected format.
pub fn toml_name_to_secret_name(
    name: impl Display,
    instance: impl Display,
    deployment: impl Display,
) -> String {
    format!("plaid-{deployment}-{instance}-{name}")
}
