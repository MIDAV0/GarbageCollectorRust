use modules::menu::menu;

use helpers::logger::init_default_logger;

mod constants;
mod modules;
mod helpers;
mod web3_client;
mod garbage_collector;
mod odos_aggregator;
mod db;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let _guard = init_default_logger();
    
    if let Err(e) = menu().await {
        tracing::error!("Execution stopped with error: {:?}", e);
    }

    Ok(())
}