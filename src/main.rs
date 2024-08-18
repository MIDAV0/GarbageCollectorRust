use eyre::Result;
use garbage_collector_rust::GarbageCollector;

enum Scenario {
    BalanceCheckerPK,
    BalanceCheckerAddressess,
}

#[tokio::main]
async fn main() -> Result<()> {
    let scenario = Scenario::BalanceCheckerPK;

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
