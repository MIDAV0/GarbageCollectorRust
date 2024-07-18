use eyre::Result;

enum Scenario {
    Scenario1,
    Scenario2,
    Scenario3,
}

#[tokio::main]
async fn main() -> Result<()> {
    let scenario = Scenario::Scenario1;

    match scenario {
        Scenario::Scenario1 => {
            println!("Scenario 1");
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
