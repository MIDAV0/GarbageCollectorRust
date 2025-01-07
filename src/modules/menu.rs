use crate::db::database::Database;
use crate::garbage_collector::garbage_collector::get_balances;

use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Select};

const LOGO: &str = r#"
  _____ _           _   _____ _ _ _ 
 |  __ (_)         | | /  ___(_) | |
 | |  \/_ _ __   __| | \ `--. _| | |
 | | __| | '_ \ / _` |  `--. \ | | |
 | |_\ \ | | | | (_| | /\__/ / | | |
  \____/_|_| |_|\__,_| \____/|_|_|_|
"#;

pub async fn menu() -> eyre::Result<()> {
    async fn read_or_create_db(with_pk: bool) -> eyre::Result<Database> {
        match Database::read(with_pk).await {
            Ok(db) => Ok(db),
            Err(_) => if with_pk { Database::new().await } else { Database::new_only_addresses().await }
        }
    }
    
    let logo = LOGO.red();

    println!("{logo}");

    loop {
        let options = vec![
            "Balance Checker with Private Keys",
            "Balance Checker with Addresses",
            "Exit",
        ];

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Choice:")
            .items(&options)
            .default(0)
            .interact()
            .unwrap();

        match selection {
            0 => {
                tracing::info!("Balance Checker with Private Keys");
                let db = read_or_create_db(true).await?;
                get_balances(db).await?;
            }
            1 => {
                tracing::info!("Balance Checker with Addresses");
                let db = read_or_create_db(false).await?;
                get_balances(db).await?;
            }
            2 => {
                return Ok(());
            }
            _ => tracing::error!("Invalid selection"),
        }
    }
}

// #[tokio::main]
// async fn main() -> eyre::Result<()> {
//     let scenario = Scenario::BalanceCheckerAddresses;

//     dotenv::dotenv().ok();
//     setup_logger().unwrap();

//     info!("Starting Garbage Collector");

//     match scenario {
//         Scenario::BalanceCheckerPK => {
//             info!("Balance Checker With Private Keys");   

//             // Parse txt file with keys
//             let keys_vec: Vec<&str> = vec![];

//             // Check if keys are empty
//             if keys_vec.is_empty() {
//                 warn!("No keys found in the file");
//                 return Ok(());
//             }

//             let mut garbage_collector = GarbageCollector::new();
//             for key in keys_vec {
//                 let parsed_signer: PrivateKeySigner = match key.parse() {
//                     Ok(signer) => signer,
//                     Err(e) => {
//                         error!("Error parsing private key {}: {:?}", key, e);
//                         continue;
//                     }
//                 };
//                 let signer_address = parsed_signer.address();
//                 garbage_collector.connect_signer(parsed_signer);
//                 if let Err(e) = garbage_collector.get_non_zero_tokens(signer_address).await {
//                     error!("Error getting non zero tokens for key {} : {:?}", key, e);
//                 }
//             }
//         }
//         Scenario::BalanceCheckerAddressess => {
//             info!("Balance Checker With Addresses");

//             // Parse txt file with addresses
//             let mut addresses_vec: Vec<&str> = vec![];

//             addresses_vec.push("0xBF17a4730Fe4a1ea36Cf536B8473Cc25ba146F19");

//             // Check if addresses are empty
//             if addresses_vec.is_empty() {
//                 warn!("No addresses found in the file");
//                 return Ok(());
//             }

//             let garbage_collector = GarbageCollector::new();
//             for address in addresses_vec {
//                 let parsed_address: Address = match address.parse() {
//                     Ok(address) => address,
//                     Err(e) => {
//                         error!("Error parsing address {}: {:?}", address, e);
//                         continue;
//                     }
//                 };
//                 if let Err(e) = garbage_collector.get_non_zero_tokens(parsed_address).await {
//                     error!("Error getting non zero tokens for address {} : {:?}", address, e);
//                 }
//             }
//         }
//         Scenario::DisplayNonZeroTokens => {
//             info!("Display Non Zero Tokens");

//             if let Err(e) = GarbageCollector::read_all_non_zero_balances() {
//                 error!("Error reading non zero balances: {:?}", e);
//             };
//         }
//     }

//     Ok(())
// }
