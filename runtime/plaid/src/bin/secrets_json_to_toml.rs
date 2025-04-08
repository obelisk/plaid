use std::io::Write;
use std::{collections::HashMap, fmt::Display};

use clap::{Arg, Command};
use serde_json::Value;

/// Take something that looks like `{plaid-secret{secret-name}}` and extract `secret-name`.
///
/// Note - This method will panic if the input string is not in the expected format.
fn json_name_to_secret_name(r: &regex::Regex, s: impl AsRef<str>) -> String {
    r.captures(s.as_ref())
        .unwrap()
        .get(1)
        .unwrap()
        .as_str()
        .to_string()
}

/// Read a file with Plaid secrets and return the content encoded in TOML.
/// Secrets follow a key/value structure where the key is the secret's name, without `{plaid-secret{}}`.
///
/// Note - This method will panic if the file does not exist or contains invalid/unexpected data.
fn json_to_toml(filename: impl Display) -> String {
    let secret_name_regex = regex::Regex::new(r"^\{plaid-secret\{([a-zA-Z0-9_-]+)\}\}$").unwrap(); // unwrap OK: hardcoded input

    // Read and parse the file's content
    let contents = std::fs::read_to_string(filename.to_string()).unwrap();
    let value = serde_json::from_str::<Value>(&contents).unwrap();

    // Collect all the secrets, ready to be re-serialized
    let secrets: HashMap<String, String> = value
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| {
            (
                json_name_to_secret_name(&secret_name_regex, k),
                v.as_str().unwrap().to_string(),
            )
        })
        .collect();

    // Serialize the instance to a TOML string
    toml::to_string(&secrets).unwrap()
}

/// Write a TOML string to a file, overwriting its content if `overwrite` is `true`.
fn toml_to_file(toml: impl Display, out_file: impl Display, overwrite: bool) {
    let out_file = out_file.to_string();
    let path = std::path::Path::new(&out_file);

    // If the file exists and we don't want to overwrite it, then exit early
    if path.exists() && !overwrite {
        println!("The file already exists. If you want to overwrite it, rerun with --overwrite");
        return;
    }

    // Write to file
    let out_string = toml.to_string();
    let mut outfile = std::fs::File::create(path).unwrap();
    writeln!(outfile, "{out_string}").unwrap();

    println!("Secrets migrated to {out_file}");
}

fn main() {
    let matches = Command::new("File Processor")
        .about("Migrates Plaid secrets from JSON to TOML format")
        .arg(
            Arg::new("in-file")
                .short('i')
                .long("in-file")
                .value_name("INPUT")
                .help("Specifies the input file")
                .required(true),
        )
        .arg(
            Arg::new("out-file")
                .short('o')
                .long("out-file")
                .value_name("OUTPUT")
                .help("Specifies the output file")
                .required(true),
        )
        .arg(
            Arg::new("overwrite")
                .long("overwrite")
                .help("Overwrite the output file if it exists")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    let in_file = matches.get_one::<String>("in-file").unwrap();
    let out_file = matches.get_one::<String>("out-file").unwrap();
    let overwrite = matches
        .get_one::<bool>("overwrite")
        .copied()
        .unwrap_or(false);

    toml_to_file(json_to_toml(in_file), out_file, overwrite);
}
