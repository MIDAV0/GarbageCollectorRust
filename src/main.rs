use eyre::Result;
use garbage_collector_rust::GarbageCollector;

use serde::{Serialize, Deserialize};
use std::{fs,path::Path};

#[derive(Serialize, Deserialize)]
struct AppConfig {
    debug: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            debug: false,
        }
    }
}

fn load_or_initialize() -> Result<AppConfig> {
    let config_path = Path::new("Config.toml");

    if config_path.exists() {
        let content = fs::read_to_string(config_path)?;
        let config = toml::from_str(&content)?;
        return Ok(config);
    }

    let config = AppConfig::default();
    let toml = toml::to_string(&config).unwrap();
    fs::write(config_path, toml)?;
    Ok(config)
}

enum Scenario {
    BalanceCheckerPK,
    BalanceCheckerAddressess,
}

#[tokio::main]
async fn main() -> Result<()> {
    let scenario = Scenario::BalanceCheckerPK;

    let config = match load_or_initialize() {
        Ok(v) => v,
        Err(err) => {
            eprintln!("Error: {}", err);
            return Err(err);
        }
    };

    match scenario {
        Scenario::BalanceCheckerPK => {
            println!("Balance Checker With Private Keys");

            // Parse txt file with keys
            let keys_vec: Vec<&str> = vec![];

            // Check if keys are empty
            if keys_vec.is_empty() {
                panic!("No keys found in the file");
            }

            let mut garbage_collector = GarbageCollector::new();
            for key in keys_vec {
                garbage_collector.connect_signer(key.parse().expect("invalid private key"));
                garbage_collector.get_non_zero_tokens(None).await?;
            }
        }
        Scenario::BalanceCheckerAddressess => {
            println!("Balance Checker With Addresses");

            // Parse txt file with addresses
            let addresses_vec: Vec<&str> = vec![];

            // Check if addresses are empty
            if addresses_vec.is_empty() {
                panic!("No addresses found in the file");
            }

            let garbage_collector = GarbageCollector::new();
            for address in addresses_vec {
                garbage_collector.get_non_zero_tokens(Some(address.to_owned())).await?;
            }
        }
    }

    Ok(())
}
