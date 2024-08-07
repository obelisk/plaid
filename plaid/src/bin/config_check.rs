use clap::{Arg, Command};
use serde_json::Value;
use sha3::{Digest, Sha3_256};

fn main() {
    let matches = Command::new("Config Check")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Confirm that secrets interpolated config is what you expect it to be")
        .arg(
            Arg::new("config")
                .help("Path to the configuration toml file")
                .long("config")
                .default_value("./plaid/resources/plaid.toml"),
        )
        .arg(
            Arg::new("secrets")
                .help("Path to the secrets json file")
                .long("secrets")
                .default_value("./plaid/private-resources/secrets.json"),
        )
        .get_matches();

    // Read the configuration file
    let mut config = std::fs::read_to_string(matches.get_one::<String>("config").unwrap())
        .expect("Failed to read configuration file");

    // Read the secrets file and parse into a serde object
    let secrets = std::fs::read(matches.get_one::<String>("secrets").unwrap())
        .expect("Failed to read secrets file");

    let secret_map = serde_json::from_slice::<Value>(&secrets)
        .unwrap()
        .as_object()
        .cloned()
        .unwrap();

    // Iterate over the secrets we just parsed and replace matching keys in the config
    for (secret, value) in secret_map {
        config = config.replace(&secret, value.as_str().unwrap());
    }

    let mut hasher = Sha3_256::new();
    hasher.update(config.as_bytes());
    let config_hash = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect::<String>();

    println!("---------- Plaid Config ----------\n{config}");
    println!("---------- Configuration Hash ----------\n{config_hash}")
}
