use std::{fmt::Display, fs::File, io::Write};

use aws_config::{BehaviorVersion, Region};
use aws_sdk_secretsmanager::{
    types::{Filter, FilterNameStringType},
    Client,
};
use clap::{Arg, ArgGroup, Command};
use serde_json::Value;

enum Operation {
    JsonToAws(String),
    AwsToJson(String),
}

struct Options {
    instance: String,
    region: String,
    operation: Operation,
}

struct PlaidSecret {
    name: String,
    value: String,
}

/// Take something that looks like `{plaid-secret{secret-name}}`
/// and extract `secret-name`. Then prepend it with `plaid-plaid-` or `plaid-ingress-`
/// depending on which instance we are processing for.
fn json_name_to_secret_name(r: &regex::Regex, instance: impl Display, s: impl Display) -> String {
    let inner_name = r
        .captures(&s.to_string())
        .unwrap()
        .get(1)
        .unwrap()
        .as_str()
        .to_string();
    format!("plaid-{instance}-{inner_name}")
}

/// Take something that looks like `plaid-plaid-<something>` or `plaid-ingress-<something>`
/// and turn it into `{plaid-secret{something}}`, which is the format expected in secrets.json
fn secret_name_to_json_name(instance: impl Display, s: impl Display) -> String {
    let input = s.to_string();
    let stripped = input.strip_prefix(&format!("plaid-{instance}-")).unwrap();
    format!("{{plaid-secret{{{stripped}}}}}")
}

/// Parse the CLI arguments
fn parse_args() -> Options {
    let matches = Command::new("Plaid Secrets Manager")
        .version("0.1.0")
        .about("A simple tool that helps with managing Plaid secrets")
        .arg(Arg::new("instance")
            .long("instance")
            .help("Specifies the type of instance (plaid or ingress)")
            .required(true)
            .value_parser(["plaid", "ingress"])
        )
        .arg(Arg::new("region")
            .long("region")
            .help("AWS region")
            .required(false)
        )
        .arg(
            Arg::new("json_to_aws")
                .long("json_to_aws")
                .help("Reads a secrets.json file and uploads secrets to AWS Secrets Manager")
                .value_name("INPUT_FILE")
        )
        .arg(
            Arg::new("aws_to_json")
                .long("aws_to_json")
                .help("Reads secrets from AWS and crafts a secrets.json file ready to be consumed by Plaid")
                .value_name("OUTPUT_FILE")
        )
        .group(
            ArgGroup::new("exclusive")
                .args(["json_to_aws", "aws_to_json"])
                .multiple(false)
                .required(true)
        )
        .get_matches();

    let instance = matches.get_one::<String>("instance").unwrap().to_string(); // unwrap OK because the param is required
    let region = matches
        .get_one::<String>("region")
        .unwrap_or(&"us-east-1".to_string())
        .to_string();
    let operation = match matches.get_one::<String>("json_to_aws") {
        Some(f) => Operation::JsonToAws(f.to_string()),
        None => {
            let filename = matches
                .get_one::<String>("aws_to_json")
                .unwrap()
                .to_string();
            Operation::AwsToJson(filename)
        }
    };

    Options {
        instance,
        region,
        operation,
    }
}

/// Read a file with Plaid secrets and upload them to AWS Secrets Manager, with appropriate names.
async fn json_to_aws(filename: impl Display, instance: impl Display, sm_client: &Client) {
    let secret_name_regex = regex::Regex::new(r"^\{plaid-secret\{([a-zA-Z0-9_-]+)\}\}$").unwrap();

    // Read and parse the file's content
    let contents = std::fs::read_to_string(filename.to_string()).unwrap();
    let value = serde_json::from_str::<Value>(&contents).unwrap();
    let value = value.as_object().unwrap();

    // Fill a vector with all the secrets, ready to be uploaded
    let mut secrets = vec![];
    for (key, value) in value {
        secrets.push(PlaidSecret {
            name: json_name_to_secret_name(&secret_name_regex, instance.to_string().clone(), key),
            value: value.as_str().unwrap().to_string(),
        });
    }

    // Upload secrets to SM
    for secret in secrets {
        println!("Uploading {}...", secret.name);
        match sm_client
            .create_secret()
            .name(secret.name.clone())
            .kms_key_id("alias/plaid-dev-encrypt-decrypt")
            .secret_string(secret.value)
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

/// Fetch secrets from AWS Secrets Manager and assemble them in a file that Plaid can consume
async fn aws_to_json(filename: impl Display, instance: impl Display, sm_client: &Client) {
    println!("Fetching all secrets whose name starts with plaid-{instance}");
    let mut next_token: Option<String> = Some("".to_string());
    let mut first_request = true;
    let mut retrieved_secrets = vec![];

    while next_token.is_some() {
        // The first request is special: we want next_token to be None
        if first_request {
            first_request = false;
            next_token = None;
        }
        let res = sm_client
            .list_secrets()
            .filters(
                Filter::builder()
                    .key(FilterNameStringType::Name)
                    .values(format!("plaid-{instance}"))
                    .build(),
            )
            .max_results(100)
            .set_next_token(next_token)
            .send()
            .await
            .unwrap();
        let secret_list = res.secret_list.unwrap();
        next_token = res.next_token;
        println!("Got {} secrets", secret_list.len());

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
    }

    // Prepare JSON Value
    let out_map: serde_json::Map<String, Value> = retrieved_secrets
        .into_iter()
        .map(|ps| {
            (
                secret_name_to_json_name(&instance, ps.name),
                Value::String(ps.value),
            )
        })
        .collect();
    let out_value: Value = Value::Object(out_map);

    // Write to file
    let out_string = serde_json::to_string(&out_value).unwrap();
    let mut outfile = File::create(&filename.to_string()).unwrap();
    writeln!(outfile, "{out_string}").unwrap();
    println!("Secrets written to {filename}");
}

#[tokio::main]
async fn main() {
    let cli_options = parse_args();
    let sdk_config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let sdk_config = sdk_config
        .to_builder()
        .region(Region::new(cli_options.region))
        .build();
    let sm_client = aws_sdk_secretsmanager::Client::new(&sdk_config);

    match cli_options.operation {
        Operation::JsonToAws(filename) => {
            json_to_aws(filename, cli_options.instance, &sm_client).await
        }
        Operation::AwsToJson(filename) => {
            aws_to_json(filename, cli_options.instance, &sm_client).await
        }
    }
}
