use crate::{db::database::Database, garbage_collector::garbage_collector::read_all_non_zero_balances};
use crate::garbage_collector::garbage_collector::{get_balances, update_token_data};

use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Select};

// Garbage collector logo
const LOGO: &str = r#"
   ______           __                        ______      ____          __            
  / ____/___ ______/ /_  ____ _____ ____     / ____/___  / / /__  _____/ /_____  _____
 / / __/ __ `/ ___/ __ \/ __ `/ __ `/ _ \   / /   / __ \/ / / _ \/ ___/ __/ __ \/ ___/
/ /_/ / /_/ / /  / /_/ / /_/ / /_/ /  __/  / /___/ /_/ / / /  __/ /__/ /_/ /_/ / /    
\____/\__,_/_/  /_.___/\__,_/\__, /\___/   \____/\____/_/_/\___/\___/\__/\____/_/     
                          /____/                                                    
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
            "Read All Non-Zero Balances",
            "Update Token Data",
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
                read_all_non_zero_balances()?;
            }
            3 => {
                tracing::info!("Updateing Token Data");
                update_token_data().await;
            }
            4 => {
                tracing::info!("Exiting");
                return Ok(());
            }
            _ => tracing::error!("Invalid selection"),
        }
    }
}