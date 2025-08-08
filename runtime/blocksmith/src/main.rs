use blocksmith::{decrypt, encrypt};
use clap::{Parser, Subcommand};
use hex::decode;

#[derive(Parser)]
#[command(name = "blocksmith", version, about = "AES-128-CBC encrypt/decrypt tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Encrypt plaintext with a 16-byte hex key
    Encrypt {
        /// Hex-encoded 16-byte key
        key_hex: String,
        /// Plaintext string to encrypt
        plaintext: String,
    },
    /// Decrypt blob with a 16-byte hex key
    Decrypt {
        /// Hex-encoded 16-byte key
        key_hex: String,
        /// Base64-encoded nonce+ciphertext blob
        ciphertext: String,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Encrypt { key_hex, plaintext } => {
            let key = decode(&key_hex).expect("Invalid key hex");
            if key.len() != 16 {
                eprintln!("Key must be 16 bytes (32 hex chars)");
                std::process::exit(1);
            }
            let blob = encrypt(&key, &plaintext).expect("Encryption failure");
            println!("{}", blob);
        }
        Commands::Decrypt {
            key_hex,
            ciphertext,
        } => {
            let key = decode(&key_hex).expect("Invalid key hex");
            if key.len() != 16 {
                eprintln!("Key must be 16 bytes (32 hex chars)");
                std::process::exit(1);
            }
            let plaintext = decrypt(&key, &ciphertext).expect("Decryption failure");
            println!("{}", plaintext);
        }
    }
}
