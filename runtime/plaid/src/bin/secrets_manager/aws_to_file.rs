use super::{utils, PlaidSecret};
use aws_sdk_secretsmanager::{
    types::{Filter, FilterNameStringType},
    Client,
};
use std::{fmt::Display, fs::File, io::Write};
use toml::Value;

/// Fetch secrets from AWS Secrets Manager and assemble them in a file that Plaid can consume.
///
/// Note - This method will panic if the data retrieved from Secrets Manager is invalid or if the file cannot be written.
pub async fn aws_to_file(
    filename: impl Display,
    instance: impl Display,
    sm_client: &Client,
    overwrite: bool,
    deployment: impl Display,
) {
    // If the file exists and we don't want to overwrite it, then exit early
    let filename = filename.to_string();
    let path = std::path::Path::new(&filename);
    if path.exists() && !overwrite {
        println!("The file already exists. If you want to overwrite it, rerun with --overwrite");
        return;
    }

    println!("Fetching all secrets whose name starts with plaid-{deployment}-{instance}");
    let mut retrieved_secrets = vec![];
    let mut next_token = None::<String>;

    loop {
        let res = sm_client
            .list_secrets()
            .filters(
                Filter::builder()
                    .key(FilterNameStringType::Name)
                    .values(format!("plaid-{deployment}-{instance}"))
                    .build(),
            )
            .max_results(100)
            .set_next_token(next_token)
            .send()
            .await
            .unwrap();
        let secret_list = res.secret_list.unwrap();
        next_token = res.next_token;

        for s in secret_list {
            // Fetch the secret value and construct a PlaidSecret object
            let value = sm_client
                .get_secret_value()
                .secret_id(s.arn.unwrap())
                .send()
                .await
                .unwrap()
                .secret_string()
                .unwrap()
                .to_string();
            retrieved_secrets.push(PlaidSecret {
                name: s.name.unwrap(),
                value,
            });
        }

        // Exit the loop if we have no more pages
        if next_token.is_none() {
            break;
        }
    }
    println!("Got {} secrets", retrieved_secrets.len());

    // Prepare TOML structure
    let out_map: toml::map::Map<String, Value> = retrieved_secrets
        .into_iter()
        .map(|ps| {
            (
                utils::strip_secret_name(&instance, &deployment, ps.name),
                Value::String(ps.value),
            )
        })
        .collect();

    // Write to file
    let out_string = toml::to_string(&out_map).unwrap();
    let mut outfile = File::create(path).unwrap();
    writeln!(outfile, "{out_string}").unwrap();
    println!("Secrets written to {filename}");
}
