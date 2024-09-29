use eyre::Result;
use garbage_collector_rust::helpers::garbage_collector::GarbageCollector;
use log::{error, info, warn};
use garbage_collector_rust::helpers::utils::setup_logger;

enum Scenario {
    BalanceCheckerPK,
    BalanceCheckerAddressess,
    DisplayNonZeroTokens,
}

#[tokio::main]
async fn main() -> Result<()> {
    let scenario = Scenario::BalanceCheckerPK;

    dotenv::dotenv().ok();
    setup_logger().unwrap();

    info!("Starting Garbage Collector");

    match scenario {
        Scenario::BalanceCheckerPK => {
            info!("Balance Checker With Private Keys");   

            // Parse txt file with keys
            let keys_vec: Vec<&str> = vec![];

            // Check if keys are empty
            if keys_vec.is_empty() {
                warn!("No keys found in the file");
                return Ok(());
            }

            let mut garbage_collector = GarbageCollector::new();
            for key in keys_vec {
                garbage_collector.connect_signer(key.parse().expect("invalid private key"));
                if let Err(e) = garbage_collector.get_non_zero_tokens(None).await {
                    error!("Error getting non zero tokens for key {} : {:?}", key, e);
                }
            }
        }
        Scenario::BalanceCheckerAddressess => {
            info!("Balance Checker With Addresses");

            // Parse txt file with addresses
            let addresses_vec: Vec<&str> = vec![];

            // Check if addresses are empty
            if addresses_vec.is_empty() {
                warn!("No addresses found in the file");
                return Ok(());
            }

            let garbage_collector = GarbageCollector::new();
            for address in addresses_vec {
                if let Err(e) = garbage_collector.get_non_zero_tokens(Some(address.to_owned())).await {
                    error!("Error getting non zero tokens for address {} : {:?}", address, e);
                }
            }
        }
        Scenario::DisplayNonZeroTokens => {
            info!("Display Non Zero Tokens");

            if let Err(e) = GarbageCollector::read_all_non_zero_balances() {
                error!("Error reading non zero balances: {:?}", e);
            };
        }
    }

    Ok(())
}
