use super::utils;
use super::PlaidSecret;
use aws_sdk_secretsmanager::Client;
use std::fmt::Display;

/// Read a file with Plaid secrets and upload them to AWS Secrets Manager, with appropriate names.
///
/// Note - This method will panic if the file does not exist or contains invalid/unexpected data.
pub async fn file_to_aws(
    filename: impl Display,
    instance: impl Display,
    sm_client: &Client,
    kms_key_id: impl Display,
    overwrite: bool,
    deployment: impl Display,
) {
    // Read and parse the file's content
    let contents = std::fs::read_to_string(filename.to_string()).unwrap();
    let contents = toml::from_str::<toml::value::Table>(&contents).unwrap();

    // Fill a vector with all the secrets, ready to be uploaded
    let secrets: Vec<PlaidSecret> = contents
        .into_iter()
        .map(|(key, value)| PlaidSecret {
            name: utils::toml_name_to_secret_name(key, instance.to_string(), &deployment),
            value: value.as_str().unwrap().to_string(),
        })
        .collect();

    // Upload secrets to SM
    for secret in secrets {
        println!("Uploading {}...", secret.name);
        match sm_client
            .create_secret()
            .name(secret.name.clone())
            .kms_key_id(kms_key_id.to_string())
            .secret_string(secret.value)
            .force_overwrite_replica_secret(overwrite)
            .send()
            .await
        {
            Ok(_) => {}
            Err(e) => {
                let err = e.into_service_error();
                // If it fails because a secret is already there, just log it but don't fail.
                // Otherwise it is a real failure.
                if err.is_resource_exists_exception() {
                    println!("Secret with name {} already exists in Secrets Manager. Skipping (NOT overwriting) it...", secret.name);
                } else {
                    panic!(
                        "Error while uploading secrets to AWS Secrets Manager: {}",
                        err
                    );
                }
            }
        }
    }
}
