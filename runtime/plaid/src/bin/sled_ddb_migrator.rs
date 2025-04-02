use aws_config::meta::region::RegionProviderChain;
use aws_config::{BehaviorVersion, Region};
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::{Client, Error};
use sled::{Db, Tree};
use std::env;

/// Migrate a sled tree by pushing all its entries to AWS DynamoDB.
///
/// Items in DynamoDB are created according to this schema:
/// * the name of the sled tree becomes the "namespace" (DDB partition key)
/// * the sled key becomes the "key" (DDB sort key)
/// * the sled value becomes the "value" (a DDB field called "value")
async fn migrate_tree(
    client: &Client,
    table_name: &str,
    namespace: &str,
    tree: &Tree,
) -> Result<(), Error> {
    println!("Migrating namespace: {namespace}");

    for item in tree.iter() {
        let (key, value) = item.expect("Failed to get key/value from tree");
        let key_str = String::from_utf8(key.to_vec()).expect("Failed to parse key");

        client
            .put_item()
            .table_name(table_name)
            .item("namespace", AttributeValue::S(namespace.to_string()))
            .item("key", AttributeValue::S(key_str))
            .item("value", AttributeValue::B(value.to_vec().into()))
            .send()
            .await?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <sled_db_path> <dynamo_table_name>", args[0]);
        std::process::exit(1);
    }

    let sled_path = &args[1];
    let table_name = &args[2];

    let db: Db = sled::open(sled_path).expect("Failed to open sled database");

    // Prepare the client to talk to DynamoDB
    let sdk_config = aws_config::load_defaults(BehaviorVersion::latest())
        .await
        .to_builder()
        .region(
            RegionProviderChain::default_provider()
                .or_else(Region::new("us-east-1"))
                .region()
                .await,
        )
        .build();
    let client = Client::new(&sdk_config);

    // Migrate the default tree (unnamed)
    let default_tree = &*db;
    migrate_tree(&client, table_name, "default", default_tree).await?;

    // Migrate all named trees
    for tree_name in db.tree_names() {
        if tree_name != b"" {
            // let name_str = bytes_to_string(&tree_name.clone().into());
            let name_str =
                String::from_utf8(tree_name.to_vec()).expect("Failed to parse tree name");
            if name_str == "__sled__default" {
                continue;
            }
            let tree = db
                .open_tree(&tree_name)
                .expect(&format!("Failed to open tree {name_str}"));
            migrate_tree(&client, table_name, &name_str, &tree).await?;
        }
    }

    println!("Migration complete.");
    Ok(())
}
