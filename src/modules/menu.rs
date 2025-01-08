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