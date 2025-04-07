use clap::{Arg, Command};
use plaid::{data::DelayedMessage, executor::Message, storage::StorageProvider};
use serde_json::Value;

/// This function takes additional parameters and produces the function that defines the data migration.
/// We use this pattern to be able to take arbitrary parameters that would not fit in the migration
/// function's signature.
fn create_migration_function(
    log_source_if_missing: String,
) -> Box<dyn Fn(String, Vec<u8>) -> (String, Vec<u8>) + Send + Sync> {
    Box::new(move |key, value| {
        // First, try to deserialize the value as a DelayedMessage. If this succeeds,
        // then this entry is in the newest format and we leave it untouched.
        if serde_json::from_slice::<DelayedMessage>(&value).is_ok() {
            // This is a message in the new format: leave it untouched
            println!(
                "Found log in the newest format. [{key}]: [{}]",
                String::from_utf8(value.clone()).unwrap()
            );
            return (key, value);
        }
        // Try to deserialize `key` as a JSON value with a `data` field.
        // If the `data` field is not there, then we assume we have a UUID:
        // this means the log is already serialized in the new format.
        // In this case, we leave the entry untouched.
        // If deserialization to a Value fails, then we don't know what to do and
        // leave everything untouched.
        match serde_json::from_str::<Value>(&key) {
            Err(_) => (key, value), // identity mapping
            Ok(mut v) => {
                if v.get("data").is_none() {
                    // `data` is missing: this is probably just a UUID: leave it
                    // Note - This shouldn't really be possible, but it does not hurt
                    return (key, value); // identity mapping
                }
                // We managed to deserialize, so now we look at the JSON fields
                if v.get("source").is_none() {
                    // `source` is missing: we add it
                    v.as_object_mut().unwrap().insert(
                        "source".to_string(),
                        serde_json::to_value(plaid_stl::messages::LogSource::Logback(
                            log_source_if_missing.clone(),
                        ))
                        .unwrap(),
                    );
                }
                if v.get("accessory_data").is_some() {
                    // `accessory_data` is present: we remove it and add `headers` and `query_params`
                    v.as_object_mut().unwrap().remove("accessory_data");
                    v.as_object_mut()
                        .unwrap()
                        .insert("headers".to_string(), Value::Object(serde_json::Map::new()));
                    v.as_object_mut().unwrap().insert(
                        "query_params".to_string(),
                        Value::Object(serde_json::Map::new()),
                    );
                }
                let id = uuid::Uuid::new_v4().to_string(); // new ID for the logback
                                                           // Insert the ID into the Map
                v.as_object_mut()
                    .unwrap()
                    .insert("id".to_string(), serde_json::to_value(id.clone()).unwrap());

                // With the changes we have made above, the value v is now basically a serialized Message.
                // We deserialize it because we need the Message itself
                let message = serde_json::from_value::<Message>(v).unwrap();

                // Now we deserialize the old value, which used to contain the delay (u64)
                let time: [u8; 8] = value.try_into().unwrap();
                let delay = u64::from_be_bytes(time);
                // We construct the DelayedMessage which will be the new value
                let delayed_message = DelayedMessage::new(delay, message);
                // Finally we serialize the DelayedMessage, ready for insertion in the DB
                let delayed_message = serde_json::to_vec(&delayed_message).unwrap();
                return (id, delayed_message);

                // match serde_json::from_slice::<u64>(&value) {
                //     Err(_) => {
                //         // Something went wrong: this is very strange. We leave the pair untouched but it should not happen
                //         return (key, value);
                //     }
                //     Ok(delay) => {
                //         // We construct the DelayedMessage which will be the new value
                //         let delayed_message = DelayedMessage::new(delay, message);
                //         // Finally we serialize the DelayedMessage, ready for insertion in the DB
                //         let delayed_message = serde_json::to_vec(&delayed_message).unwrap();
                //         return (id, delayed_message);
                //     }
                // }
            }
        }
    })
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    let matches = Command::new("plaid_db_migrator")
        .version("0.22.2")
        .about("Tool to apply a data migration (written as a Rust function) to a Plaid DB")
        .arg(
            Arg::new("db_type")
                .long("db")
                .help("The type of DB to apply the migration on")
                .value_parser(["sled", "dynamodb"])
                .value_name("TYPE")
                .required(true)
                .num_args(1),
        )
        .arg(
            Arg::new("sled_db_path")
                .long("path")
                .value_name("PATH")
                .help("Path to the sled database")
                .required_if_eq("db_type", "sled"),
        )
        .arg(
            Arg::new("dynamo_table_name")
                .long("table-name")
                .value_name("TABLE")
                .help("DynamoDB table name")
                .required_if_eq("db_type", "dynamodb")
                .conflicts_with("sled_db_path"),
        )
        .arg(
            Arg::new("namespace")
                .long("namespace")
                .help("DB namespace to apply the migration on")
                .value_name("NS")
                .required(true),
        )
        .arg(
            Arg::new("logsource")
                .long("logsource")
                .help("Log source to use if missing in the DB")
                .value_name("SOURCE"),
        )
        .get_matches();

    let db_type = matches.get_one::<String>("db_type").unwrap();
    let namespace = matches.get_one::<String>("namespace").unwrap();
    let missing_logsource = "missing_logsource".to_string();
    let logsource = matches
        .get_one::<String>("logsource")
        .unwrap_or(&missing_logsource);

    match db_type.as_str() {
        "sled" => {
            let sled_path = matches.get_one::<String>("sled_db_path").unwrap();
            apply_sled_migration(sled_path, namespace, logsource)
                .await
                .unwrap();
        }
        "dynamodb" => {
            let table_name = matches.get_one::<String>("dynamo_table_name").unwrap();
            apply_dynamodb_migration(table_name, namespace, logsource)
                .await
                .unwrap();
        }
        _ => unreachable!("Unknown DB type {db_type}"),
    }

    println!("Migration complete.");
    Ok(())
}

async fn apply_sled_migration(path: &str, namespace: &str, logsource: &str) -> Result<(), ()> {
    let config = plaid::storage::sled::Config {
        sled_path: path.to_string(),
    };
    let sled = plaid::storage::sled::Sled::new(config).unwrap();
    sled.apply_migration(namespace, create_migration_function(logsource.to_string()))
        .await
        .unwrap();
    Ok(())
}

async fn apply_dynamodb_migration(
    table_name: &str,
    namespace: &str,
    logsource: &str,
) -> Result<(), ()> {
    let config = plaid::storage::dynamodb::Config {
        authentication: plaid::AwsAuthentication::Iam {},
        table_name: table_name.to_string(),
    };
    let dynamodb = plaid::storage::dynamodb::DynamoDb::new(config).await;
    dynamodb
        .apply_migration(namespace, create_migration_function(logsource.to_string()))
        .await
        .unwrap();
    Ok(())
}
