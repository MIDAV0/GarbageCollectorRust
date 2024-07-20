use eyre::Result;
use garbage_collector_rust::GarbageCollector;

enum Scenario {
    BalanceChecker,
    Scenario2,
    Scenario3,
}

#[tokio::main]
async fn main() -> Result<()> {
    let scenario = Scenario::BalanceChecker;

    match scenario {
        Scenario::BalanceChecker => {
            println!("Balance Checker");

            // Parse txt file with keys
            let keys_vec: Vec<&str> = vec![];

            // Check if keys are empty
            if keys_vec.is_empty() {
                panic!("No keys found in the file");
            }

            let mut garbage_collector = GarbageCollector::new();
            for key in keys_vec {
                garbage_collector.connect_signer(key.parse().expect("invalid private key"));
            }
        }
        Scenario::Scenario2 => {
            println!("Scenario 2");
        }
        Scenario::Scenario3 => {
            println!("Scenario 3");
        }
    }

    Ok(())
}
