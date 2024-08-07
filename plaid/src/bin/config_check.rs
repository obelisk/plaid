use clap::{Arg, Command};
use plaid::config::read_and_interpolate;

fn main() {
    env_logger::init();
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

    let config_path = matches.get_one::<String>("config").unwrap();
    let secrets_path = matches.get_one::<String>("secrets").unwrap();

    read_and_interpolate(config_path, secrets_path, true).expect("Invalid config!");
}
