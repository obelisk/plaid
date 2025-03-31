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
    JsonToAws(String, String, String),
    AwsToJson(String, String),
}

/// CLI parameters
struct Options {
    instance: String,
    region: String,
    operation: Operation,
    overwrite: bool,
}

/// A secret used by the Plaid system
struct PlaidSecret {
    name: String,
    value: String,
}

/// Take something that looks like `{plaid-secret{secret-name}}`
/// and extract `secret-name`. Then prepend it with `plaid-<deployment>-plaid-` or `plaid-<deployment>-ingress-`
/// depending on which deployment and instance we are processing for.
///
/// Note - This method will panic if the input string is not in the expected format.
fn json_name_to_secret_name(
    r: &regex::Regex,
    instance: impl Display,
    deployment: impl Display,
    s: impl AsRef<str>,
) -> String {
    let inner_name = r
        .captures(s.as_ref())
        .unwrap()
        .get(1)
        .unwrap()
        .as_str()
        .to_string();
    format!("plaid-{deployment}-{instance}-{inner_name}")
}

/// Take something that looks like `plaid-<deployment>-plaid-<something>` or `plaid-<deployment>-ingress-<something>`
/// and turn it into `{plaid-secret{something}}`, which is the format expected in secrets.json
///
/// Note - This method will panic if the input string is not in the expected format.
fn secret_name_to_json_name(
    instance: impl Display,
    deployment: impl Display,
    s: impl Display,
) -> String {
    let input = s.to_string();
    let stripped = input
        .strip_prefix(&format!("plaid-{deployment}-{instance}-"))
        .unwrap();
    format!("{{plaid-secret{{{stripped}}}}}")
}

/// Parse the CLI arguments
fn parse_args() -> Options {
    let matches = Command::new("Plaid Secrets Manager")
        .version("0.20.1")
        .about("A simple tool that helps with managing Plaid secrets")
        .subcommand_required(true)
        .subcommand(
            Command::new("aws_to_json")
                .about("Reads secrets from AWS and crafts a file ready to be consumed by Plaid"),
        )
        .subcommand(
            Command::new("json_to_aws")
                .about("Reads a secrets file and uploads secrets to AWS Secrets Manager")
                .arg(
                    Arg::new("kms_key_id")
                        .long("kms_key_id")
                        .help(
                            "ID of the KMS key used to encrypt secrets uploaded to Secrets Manager",
                        )
                        .required(false)
                        .default_value("alias/plaid-dev-encrypt-decrypt"),
                ),
        )
        .arg(
            Arg::new("filename")
                .long("filename")
                .help("The name of the file to read from or write to")
                .required(true),
        )
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
            Arg::new("overwrite")
                .long("overwrite")
                .help("Warning - Overwrite secrets or files with same name")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("deployment")
                .long("deployment")
                .help("The deployment that this Plaid instance belongs to")
                .required(true),
        )
        .group(
            ArgGroup::new("instance")
                .args(["plaid", "ingress", "other"])
                .multiple(false)
                .required(true),
        )
        .get_matches();

    let region = matches.get_one::<String>("region").unwrap().to_string(); // unwrap OK: it has a default value
    let overwrite = matches.get_one::<bool>("overwrite").unwrap(); // unwrap OK: defaults to false
    let filename = matches.get_one::<String>("filename").unwrap().to_string(); // unwrap OK: it's required
    let deployment = matches.get_one::<String>("deployment").unwrap().to_string(); // unwrap OK: it's required

    let (subcmd_name, subcmd_args) = matches.subcommand().unwrap(); // OK: subcommand is required
    let operation = match subcmd_name {
        "json_to_aws" => Operation::JsonToAws(
            filename,
            subcmd_args
                .get_one::<String>("kms_key_id")
                .unwrap() // OK: it has a default value
                .to_string(),
            deployment,
        ),
        "aws_to_json" => Operation::AwsToJson(filename, deployment),
        _ => unreachable!(), // those above are the only valid subcommands
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
        operation,
        overwrite: *overwrite,
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
    overwrite: bool,
    deployment: impl Display,
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
            name: json_name_to_secret_name(
                &secret_name_regex,
                instance.to_string(),
                &deployment,
                key,
            ),
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

/// Fetch secrets from AWS Secrets Manager and assemble them in a file that Plaid can consume.
///
/// Note - This method will panic if the data retrieved from Secrets Manager is invalid or if the file cannot be written.
async fn aws_to_json(
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
                secret_name_to_json_name(&instance, &deployment, ps.name),
                Value::String(ps.value),
            )
        })
        .collect();
    let out_value: Value = Value::Object(out_map);

    // Write to file
    let out_string = serde_json::to_string(&out_value).unwrap();
    let mut outfile = File::create(path).unwrap();
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
        Operation::JsonToAws(filename, kms_key_id, deployment) => {
            json_to_aws(
                filename,
                cli_options.instance,
                &sm_client,
                kms_key_id,
                cli_options.overwrite,
                deployment,
            )
            .await
        }
        Operation::AwsToJson(filename, deployment) => {
            aws_to_json(
                filename,
                cli_options.instance,
                &sm_client,
                cli_options.overwrite,
                deployment,
            )
            .await
        }
    }
}
