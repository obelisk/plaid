use super::{Operation, Options};
use clap::{Arg, ArgAction, ArgGroup, Command, Id};

/// Parse the CLI arguments
pub fn parse_args() -> Options {
    // Define all the arguments that will be shared between the sub-commands.
    // This allows us to use the more typical structure
    // program sub_command --args
    // instead of
    // program --args sub_command

    let arg_filename = Arg::new("filename")
        .long("filename")
        .help("The name of the file to read from or write to")
        .required(true);

    let arg_plaid = Arg::new("plaid")
        .long("plaid")
        .action(ArgAction::SetTrue)
        .help("Operate on the plaid instance (i.e., the one not exposed to the internet)");

    let arg_ingress = Arg::new("ingress")
        .long("ingress")
        .action(ArgAction::SetTrue)
        .help("Operate on the ingress instance (i.e., the one exposed to the internet)");

    let arg_other = Arg::new("other")
        .long("other")
        .value_name("INSTANCE")
        .help("Operate on another type of instance, to be specified");

    let arg_region = Arg::new("region")
        .long("region")
        .help("AWS region")
        .required(false)
        .default_value("us-east-1");

    let arg_overwrite = Arg::new("overwrite")
        .long("overwrite")
        .help("Warning - Overwrite secrets or files with same name")
        .action(ArgAction::SetTrue);

    let arg_deployment = Arg::new("deployment")
        .long("deployment")
        .help("The deployment that this Plaid instance belongs to")
        .required(true);

    // Now define the main CLI
    let matches = Command::new("Plaid Secrets Manager")
        .version("0.23.2")
        .about("A simple tool that helps with managing Plaid secrets")
        .subcommand_required(true)
        .subcommand(
            Command::new("aws_to_file")
                .about("Reads secrets from AWS and crafts a file ready to be consumed by Plaid")
                .arg(arg_filename.clone())
                .arg(arg_plaid.clone())
                .arg(arg_ingress.clone())
                .arg(arg_other.clone())
                .arg(arg_region.clone())
                .arg(arg_overwrite.clone())
                .arg(arg_deployment.clone())
                .group(
                    ArgGroup::new("instance")
                        .args(["plaid", "ingress", "other"])
                        .multiple(false)
                        .required(true),
                ),
        )
        .subcommand(
            Command::new("file_to_aws")
                .about("Reads a secrets file and uploads secrets to AWS Secrets Manager")
                .arg(
                    Arg::new("kms_key_id")
                        .long("kms_key_id")
                        .help(
                            "ID of the KMS key used to encrypt secrets uploaded to Secrets Manager",
                        )
                        .required(false)
                        .default_value("alias/plaid-dev-encrypt-decrypt"),
                )
                .arg(arg_filename.clone())
                .arg(arg_plaid.clone())
                .arg(arg_ingress.clone())
                .arg(arg_other.clone())
                .arg(arg_region.clone())
                .arg(arg_overwrite.clone())
                .arg(arg_deployment.clone())
                .group(
                    ArgGroup::new("instance")
                        .args(["plaid", "ingress", "other"])
                        .multiple(false)
                        .required(true),
                ),
        )
        .get_matches();

    let (subcmd_name, subcmd_args) = matches.subcommand().unwrap(); // OK: subcommand is required

    let region = subcmd_args.get_one::<String>("region").unwrap().to_string(); // unwrap OK: it has a default value
    let overwrite = subcmd_args.get_one::<bool>("overwrite").unwrap(); // unwrap OK: defaults to false
    let filename = subcmd_args
        .get_one::<String>("filename")
        .unwrap()
        .to_string(); // unwrap OK: it's required
    let deployment = subcmd_args
        .get_one::<String>("deployment")
        .unwrap()
        .to_string(); // unwrap OK: it's required
    let instance_id = subcmd_args.get_one::<Id>("instance").unwrap().as_str();
    let instance = match instance_id {
        "plaid" | "ingress" => instance_id.to_string(),
        "other" => matches.get_one::<String>(instance_id).unwrap().to_string(),
        _ => unreachable!(),
    };

    // Now that we have all the arguments, build the Operation depending on the subcommand
    let operation = match subcmd_name {
        "file_to_aws" => Operation::FileToAws(
            filename,
            subcmd_args
                .get_one::<String>("kms_key_id")
                .unwrap() // OK: it has a default value
                .to_string(),
            deployment,
        ),
        "aws_to_file" => Operation::AwsToFile(filename, deployment),
        _ => unreachable!(), // those above are the only valid subcommands
    };

    Options {
        instance,
        region,
        operation,
        overwrite: *overwrite,
    }
}
