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

        // If we don't want to overwrite existing secrets, just try to create and continue on
        if !overwrite {
            create_secret(&sm_client, &secret, &kms_key_id).await;
            continue;
        }

        // If in overwrite mode, first get the ARN of the secret from Secrets Manager. If we cannot find an ARN
        // for the secret, we'll just create the secret and continue
        let secret_id = match sm_client
            .get_secret_value()
            .secret_id(&secret.name)
            .send()
            .await
        {
            Ok(response) => {
                let Some(arn) = response.arn else {
                    eprintln!(
                        "No ARN present in response from Secrets Manager for {}. Skipping...",
                        secret.name
                    );
                    continue;
                };
                arn
            }
            Err(e) => {
                let err = e.into_service_error();
                if err.is_resource_not_found_exception() {
                    println!(
                        "No existing secret found for {}. Creating a new one...",
                        &secret.name
                    );
                    create_secret(&sm_client, &secret, &kms_key_id).await;
                    continue;
                }

                eprintln!(
                    "Failed to get ARN of {} from Secrets Manager and cannot update its value. Error: {err}",
                    &secret.name
                );
                continue;
            }
        };

        let response = sm_client
            .update_secret()
            .secret_id(secret_id)
            .kms_key_id(kms_key_id.to_string())
            .secret_string(secret.value)
            .send()
            .await;

        if let Err(e) = response {
            eprintln!(
                "Failed to update secret value for {}. Error: {e}",
                secret.name
            )
        }
    }
}

async fn create_secret(client: &Client, secret: &PlaidSecret, kms_key_id: &impl Display) {
    let response = client
        .create_secret()
        .name(&secret.name)
        .kms_key_id(kms_key_id.to_string())
        .secret_string(&secret.value)
        .send()
        .await;

    if let Err(e) = response {
        let err = e.into_service_error();
        // If it fails because a secret is already there, just log it but don't fail.
        // Otherwise it is a real failure.
        if err.is_resource_exists_exception() {
            println!("Secret with name {} already exists in Secrets Manager. Skipping (NOT overwriting) it...", secret.name);
        } else {
            panic!(
                "Error while uploading {} to AWS Secrets Manager. Error: {err}",
                secret.name
            );
        }
    }
}
