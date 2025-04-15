mod aws_to_file;
mod cli;
mod file_to_aws;
mod utils;

use aws_config::{BehaviorVersion, Region};

/// The operation we are performing. It can be
/// * Reading secrets from a file and uploading them to Secrets Manager
/// * Fetching secrets from Secrets Manager and writing them to a file
enum Operation {
    FileToAws(String, String, String),
    AwsToFile(String, String),
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

#[tokio::main]
async fn main() {
    let cli_options = cli::parse_args();

    // Prepare the client to talk to AWS Secrets Manager
    let sdk_config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let sdk_config = sdk_config
        .to_builder()
        .region(Region::new(cli_options.region))
        .build();
    let sm_client = aws_sdk_secretsmanager::Client::new(&sdk_config);

    match cli_options.operation {
        Operation::FileToAws(filename, kms_key_id, deployment) => {
            file_to_aws::file_to_aws(
                filename,
                cli_options.instance,
                &sm_client,
                kms_key_id,
                cli_options.overwrite,
                deployment,
            )
            .await
        }
        Operation::AwsToFile(filename, deployment) => {
            aws_to_file::aws_to_file(
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
