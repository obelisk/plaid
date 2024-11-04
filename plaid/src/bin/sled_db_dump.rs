use std::collections::HashMap;

use clap::{Arg, Command};
use plaid::executor::Message;
use serde::{Deserialize, Serialize};
use sled::IVec;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyMessage {
    pub type_: String,
    pub data: Vec<u8>,
    pub accessory_data: HashMap<String, Vec<u8>>,
}

fn print_lb_log(key: &[u8], value: &[u8]) {
    let time = u64::from_be_bytes(value.try_into().unwrap());

    match serde_json::from_slice::<Message>(key) {
        Ok(msg) => println!(
            "Time: {time} Type: {} Data: {:?}",
            msg.type_,
            String::from_utf8(msg.data)
        ),
        Err(_e) => {
            // Try legacy message
            match serde_json::from_slice::<LegacyMessage>(key) {
                Ok(msg) => println!(
                    "LEGACY Time: {time} Type: {} Data: {:?}",
                    msg.type_,
                    String::from_utf8(msg.data)
                ),
                Err(_e) => {
                    println!("Skipping log in storage system which could not be deserialized")
                }
            }
        }
    };
}

fn main() {
    env_logger::init();
    let matches = Command::new("Sled DB Dump")
        .version(env!("CARGO_PKG_VERSION"))
        .about("See what's stored inside your sled database")
        .arg(
            Arg::new("db")
                .help("Path to the Sled DB folder")
                .long("db")
                .default_value("/opt/plaid/sled"),
        )
        .arg(
            Arg::new("only-logback")
                .help("Only print logback entries")
                .long("lb")
                .num_args(0),
        )
        .get_matches();

    let db_path = matches.get_one::<String>("db").unwrap();

    let db: sled::Db = sled::open(&db_path).unwrap();

    let tree_names = match matches.get_flag("only-logback") {
        true => vec![IVec::from("logback_internal")],
        false => db.tree_names(),
    };

    for tree_name in tree_names {
        let name = String::from_utf8(tree_name.to_vec()).unwrap();
        println!("Tree: {name}");

        let tree = db.open_tree(tree_name).expect("Failed opening tree");

        // The use of a filter_map here means keys that fail to be pulled will be thrown away.
        // I don't know if this is possible? Maybe if the database is moved out from under us?
        let data: Vec<(Vec<u8>, Vec<u8>)> = tree
            .iter()
            .filter_map(|x| match x {
                Ok((k, v)) => Some((k.to_vec(), v.to_vec())),
                Err(e) => panic!("Storage Error Listing Keys: {e}"),
            })
            .collect();

        for (k, v) in data {
            match name.as_str() {
                "logback_internal" => print_lb_log(&k, &v),
                _ => println!(
                    "\tKey: {:?}, Value: {:?}",
                    String::from_utf8(k.to_vec()),
                    String::from_utf8(v.to_vec())
                ),
            }
        }
    }
}
