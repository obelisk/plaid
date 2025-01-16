use std::{fmt::Display, fs::File, io::Write};

use aws_config::{BehaviorVersion, Region};
use aws_sdk_secretsmanager::{
    types::{Filter, FilterNameStringType},
    Client,
};
use clap::{Arg, ArgAction, ArgGroup, Command, Id};
use serde_json::Value;

/// The operation we are performing. It can be
/// * Reading secrets from a JSON file and uploading them to Secrets Manager
/// * Fetching secrets from Secrets Manager and writing them to a JSON file
enum Operation {
    JsonToAws(String),
    AwsToJson(String),
}

/// CLI parameters
struct Options {
    instance: String,
    region: String,
    kms_key_id: String,
    operation: Operation,
}

/// A secret used by the Plaid system
struct PlaidSecret {
    name: String,
    value: String,
}

/// Take something that looks like `{plaid-secret{secret-name}}`
/// and extract `secret-name`. Then prepend it with `plaid-plaid-` or `plaid-ingress-`
/// depending on which instance we are processing for.
///
/// Note - This method will panic if the input string is not in the expected format.
fn json_name_to_secret_name(
    r: &regex::Regex,
    instance: impl Display,
    s: impl AsRef<str>,
) -> String {
    let inner_name = r
        .captures(s.as_ref())
        .unwrap()
        .get(1)
        .unwrap()
        .as_str()
        .to_string();
    format!("plaid-{instance}-{inner_name}")
}

/// Take something that looks like `plaid-plaid-<something>` or `plaid-ingress-<something>`
/// and turn it into `{plaid-secret{something}}`, which is the format expected in secrets.json
///
/// Note - This method will panic if the input string is not in the expected format.
fn secret_name_to_json_name(instance: impl Display, s: impl Display) -> String {
    let input = s.to_string();
    let stripped = input.strip_prefix(&format!("plaid-{instance}-")).unwrap();
    format!("{{plaid-secret{{{stripped}}}}}")
}

/// Parse the CLI arguments
fn parse_args() -> Options {
    let matches = Command::new("Plaid Secrets Manager")
        .version("0.14.0")
        .about("A simple tool that helps with managing Plaid secrets")
        .arg(
            Arg::new("plaid")
                .long("plaid")
                .action(ArgAction::SetTrue)
                .help("Operate on the plaid instance (i.e., the one not exposed to the internet)"),
        )
        .arg(
            Arg::new("ingress")
                .long("ingress")
                .action(ArgAction::SetTrue)
                .help("Operate on the ingress instance (i.e., the one exposed to the internet)"),
        )
        .arg(
            Arg::new("other")
                .long("other")
                .value_name("INSTANCE")
                .help("Operate on another type of instance, to be specified"),
        )
        .arg(
            Arg::new("region")
                .long("region")
                .help("AWS region")
                .required(false)
                .default_value("us-east-1"),
        )
        .arg(
            Arg::new("kms_key_id")
                .long("kms_key_id")
                .help("ID of the KMS key used to encrypt secrets uploaded to Secrets Manager")
                .required(false)
                .default_value("alias/plaid-dev-encrypt-decrypt"),
        )
        .arg(
            Arg::new("json_to_aws")
                .long("json_to_aws")
                .help("Reads a secrets file and uploads secrets to AWS Secrets Manager")
                .value_name("INPUT_FILE"),
        )
        .arg(
            Arg::new("aws_to_json")
                .long("aws_to_json")
                .help("Reads secrets from AWS and crafts a file ready to be consumed by Plaid")
                .value_name("OUTPUT_FILE"),
        )
        .group(
            ArgGroup::new("action")
                .args(["json_to_aws", "aws_to_json"])
                .multiple(false)
                .required(true),
        )
        .group(
            ArgGroup::new("instance")
                .args(["plaid", "ingress", "other"])
                .multiple(false)
                .required(true),
        )
        .get_matches();

    let region = matches.get_one::<String>("region").unwrap().to_string(); // unwrap OK because it has a default value
    let kms_key_id = matches.get_one::<String>("kms_key_id").unwrap().to_string(); // unwrap OK because it has a default value

    let operation_id = matches.get_one::<Id>("action").unwrap().as_str();
    let operation = match operation_id {
        "json_to_aws" => {
            let filename = matches.get_one::<String>(operation_id).unwrap().to_string();
            Operation::JsonToAws(filename)
        }
        "aws_to_json" => {
            let filename = matches.get_one::<String>(operation_id).unwrap().to_string();
            Operation::AwsToJson(filename)
        }
        _ => unreachable!(), // impossible: only the values above are accepted
    };

    let instance_id = matches.get_one::<Id>("instance").unwrap().as_str();
    let instance = match instance_id {
        "plaid" | "ingress" => instance_id.to_string(),
        "other" => matches.get_one::<String>(instance_id).unwrap().to_string(),
        _ => unreachable!(),
    };

    Options {
        instance,
        region,
        kms_key_id,
        operation,
    }
}

/// Read a file with Plaid secrets and upload them to AWS Secrets Manager, with appropriate names.
///
/// Note - This method will panic if the file does not exist or contains invalid/unexpected data.
async fn json_to_aws(
    filename: impl Display,
    instance: impl Display,
    sm_client: &Client,
    kms_key_id: impl Display,
) {
    let secret_name_regex = regex::Regex::new(r"^\{plaid-secret\{([a-zA-Z0-9_-]+)\}\}$").unwrap(); // unwrap OK: hardcoded input

    // Read and parse the file's content
    let contents = std::fs::read_to_string(filename.to_string()).unwrap();
    let value = serde_json::from_str::<Value>(&contents).unwrap();
    let value = value.as_object().unwrap();

    // Fill a vector with all the secrets, ready to be uploaded
    let mut secrets = vec![];
    for (key, value) in value {
        secrets.push(PlaidSecret {
            name: json_name_to_secret_name(&secret_name_regex, instance.to_string(), key),
            value: value.as_str().unwrap().to_string(),
        });
    }

    // Upload secrets to SM
    for secret in secrets {
        println!("Uploading {}...", secret.name);
        match sm_client
            .create_secret()
            .name(secret.name.clone())
            .kms_key_id(kms_key_id.to_string())
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

/// Fetch secrets from AWS Secrets Manager and assemble them in a file that Plaid can consume.
///
/// Note - This method will panic if the data retrieved from Secrets Manager is invalid or if the file cannot be written.
async fn aws_to_json(filename: impl Display, instance: impl Display, sm_client: &Client) {
    println!("Fetching all secrets whose name starts with plaid-{instance}");
    let mut retrieved_secrets = vec![];
    let mut next_token = None::<String>;

    loop {
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

        // Exit the loop if we have no more pages
        if next_token.is_none() {
            break;
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
            json_to_aws(
                filename,
                cli_options.instance,
                &sm_client,
                cli_options.kms_key_id,
            )
            .await
        }
        Operation::AwsToJson(filename) => {
            aws_to_json(filename, cli_options.instance, &sm_client).await
        }
    }
}
