use std::cmp::Ordering;

use clap::{Arg, Command};
use plaid::{data::DelayedMessage, storage::StorageProvider};

/// A key/value entry in the DB.
struct DbEntry {
    key: String,
    value: DelayedMessage,
}

/// Temporary data structure that we use when
/// we want to sort logbacks before printing them.
struct TmpLogback {
    /// The number of seconds between now and the execution time
    delay: u64,
    /// The formatted string we will print out
    output: String,
}

impl PartialEq for TmpLogback {
    fn eq(&self, other: &Self) -> bool {
        self.delay == other.delay && self.output == other.output
    }
}

impl Eq for TmpLogback {}

impl PartialOrd for TmpLogback {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(&other))
    }
}

impl Ord for TmpLogback {
    fn cmp(&self, other: &Self) -> Ordering {
        self.delay.cmp(&other.delay)
    }
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
    let dynamodb = plaid::storage::dynamodb::DynamoDb::new(config)
        .await
        .unwrap();
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
        .arg(
            Arg::new("filter_max_delay")
                .long("max-delay")
                .value_parser(clap::value_parser!(u64))
                .help("Filter by max delay (less-than-or-equal)"),
        )
        .arg(
            Arg::new("sort_asc")
                .long("sort")
                .action(clap::ArgAction::SetTrue)
                .help("Sort by delay, in ascending order"),
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
    let filter_max_delay = matches.get_one::<u64>("filter_max_delay");
    let sort_asc = matches.get_one::<bool>("sort_asc").unwrap().clone();

    let all_data = fetch_all(table_name, namespace)
        .await
        .expect("Could not fetch data from DynamoDB");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();

    // If we are printing sorted output, this vec
    // will be used for storing unsorted items.
    let mut to_sort = vec![];

    for item in all_data {
        if let Ok(db_entry) = DbEntry::try_from(item) {
            let msg = db_entry.value.message;
            let exec_delay = db_entry.value.delay - now;
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
                if let Some(filter_max_delay) = filter_max_delay {
                    if exec_delay > *filter_max_delay {
                        continue;
                    }
                }

                // If we are here, then we keep it: prepare the output string
                let output = format!(
                    "[{:-<20}] [{:-<15}] [{:-<20}] [{:-<40}] {}",
                    db_entry.key,
                    format!("in {} s", exec_delay),
                    msg.type_,
                    source,
                    data
                );

                if !sort_asc {
                    // We are not trying to sort the output, so we just print it and proceed.
                    println!("{output}");
                } else {
                    // We want to sort, so we store it without printing. Then we will sort and print.
                    to_sort.push(TmpLogback {
                        delay: exec_delay,
                        output,
                    });
                }
            }
        }
    }

    // At the end of the for-loop, if we wanted to print sorted logbacks, we process the vector and print.
    if sort_asc {
        to_sort.sort();
        for item in to_sort {
            println!("{}", item.output);
        }
    }
}
