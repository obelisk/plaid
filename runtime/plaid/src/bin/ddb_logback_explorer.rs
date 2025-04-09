use clap::{Arg, Command};
use plaid::{data::DelayedMessage, storage::StorageProvider};

/// A key/value entry in the DB.
struct DbEntry {
    key: String,
    value: DelayedMessage,
}

impl TryFrom<(String, Vec<u8>)> for DbEntry {
    type Error = String;

    fn try_from(input: (String, Vec<u8>)) -> Result<Self, Self::Error> {
        let value =
            String::from_utf8(input.1).map_err(|_| "The value is not printable".to_string())?;
        let delayed_message = serde_json::from_str::<DelayedMessage>(&value)
            .map_err(|_| "Failed to parse DelayedMessage".to_string())?;
        Ok(DbEntry {
            key: input.0,
            value: delayed_message,
        })
    }
}

/// Get all the data (keys and values) from a given namespace.
async fn fetch_all(table_name: &str, namespace: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let config = plaid::storage::dynamodb::Config {
        authentication: plaid::AwsAuthentication::Iam {},
        table_name: table_name.to_string(),
    };
    let dynamodb = plaid::storage::dynamodb::DynamoDb::new(config).await;
    dynamodb
        .fetch_all(namespace, None)
        .await
        .map_err(|e| format!("Error while fetching data from DynamoDB: {e}"))
}

#[tokio::main]
async fn main() {
    let matches = Command::new("ddb_logback_explorer")
        .version("0.23.0")
        .about("CLI that helps exploring Plaid's logbacks when using DynamoDB")
        .arg(
            Arg::new("table")
                .short('t')
                .long("table-name")
                .help("The name of the DynamoDB table")
                .required(true)
                .value_parser(clap::value_parser!(String)),
        )
        .arg(
            Arg::new("namespace")
                .short('n')
                .long("namespace")
                .help("The namespace in DynamoDB")
                .required(true)
                .value_parser(clap::value_parser!(String)),
        )
        .arg(
            Arg::new("filter_type")
                .long("type")
                .help("Filter by log type (exact match)"),
        )
        .arg(
            Arg::new("filter_source")
                .long("source")
                .help("Filter by log source (substring)"),
        )
        .arg(
            Arg::new("filter_data")
                .long("data")
                .help("Filter by log data (substring)"),
        )
        .get_matches();

    let table_name = matches
        .get_one::<String>("table")
        .expect("Table argument missing");
    let namespace = matches
        .get_one::<String>("namespace")
        .expect("Namespace argument missing");
    let filter_type = matches.get_one::<String>("filter_type");
    let filter_source = matches.get_one::<String>("filter_source");
    let filter_data = matches.get_one::<String>("filter_data");

    let all_data = fetch_all(table_name, namespace)
        .await
        .expect("Could not fetch data from DynamoDB");

    for item in all_data {
        if let Ok(db_entry) = DbEntry::try_from(item) {
            let msg = db_entry.value.message;
            if let Ok(data) = String::from_utf8(msg.data) {
                let source = msg.source.to_string();

                // Check if we should print this entry or not, according to the filters
                if let Some(filter_type) = filter_type {
                    if filter_type.as_str() != msg.type_ {
                        continue;
                    }
                }
                if let Some(filter_source) = filter_source {
                    if !source.contains(filter_source) {
                        continue;
                    }
                }
                if let Some(filter_data) = filter_data {
                    if !data.contains(filter_data) {
                        continue;
                    }
                }
                // If we are here, then we print it
                println!(
                    "[{:-<20}] [{:-<20}] [{:-<40}] {}",
                    db_entry.key, msg.type_, source, data
                );
            }
        }
    }
}
