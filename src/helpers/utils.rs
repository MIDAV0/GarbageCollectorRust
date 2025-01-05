use log::LevelFilter;
use fern::{Dispatch, colors::{Color, ColoredLevelConfig}};
use eyre::Result;
use serde_json::{to_string_pretty, Value};
use std::{future::Future, time, fs, io::Write};
use alloy::{
    contract::Error as ContractError, network::{Ethereum, EthereumWallet}, primitives::Address, providers::{
        fillers::{FillProvider, JoinFill, RecommendedFiller, WalletFiller}, ProviderBuilder, RootProvider
    }, transports::http::Http
};
use alloy_json_rpc::RpcError;
use reqwest::{Client, Url};
use tokio::io::AsyncBufReadExt;

use crate::constants::{Network, CHAINS_FILE_PATH, PROJECT_NAME};


pub type MyFiller = FillProvider<JoinFill<RecommendedFiller, WalletFiller<EthereumWallet>>, RootProvider<Http<Client>>, Http<Client>, Ethereum>;

pub async fn read_file_lines(path: &str) -> eyre::Result<Vec<String>> {
    let file = tokio::fs::read(path).await?;
    let mut lines = file.lines();

    let mut lines_vec = vec![];
    while let Some(line) = lines.next_line().await? {
        lines_vec.push(line)
    }

    Ok(lines_vec)
}

pub fn setup_logger() -> Result<()> {
    let colors = ColoredLevelConfig {
        trace: Color::Cyan,
        debug: Color::Magenta,
        info: Color::Green,
        warn: Color::Red,
        error: Color::BrightRed,
        ..ColoredLevelConfig::new()
    };

    Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{}[{}] {}",
                chrono::Local::now().format("[%H:%M:%S]"),
                colors.color(record.level()),
                message
            ))
        })
        .chain(std::io::stdout())
        .level(log::LevelFilter::Info)
        .level_for(PROJECT_NAME, LevelFilter::Info)
        .apply()?;

    Ok(())
}

// Parse JSON file
pub fn parse_json_data(file_path: &str) -> Result<Value> {
    let contents = fs::read_to_string(file_path)?;
    let v: Value = serde_json::from_str(&contents)?;
    Ok(v)
}

pub fn get_networks() -> Result<Vec<Network>> {
    let chain_data = parse_json_data(CHAINS_FILE_PATH)?;
    let mut networks = vec![];
    for (k, v) in chain_data.as_object().unwrap() {
        match Network::new( 
            v["id"].as_u64().unwrap() as u32,
            k.clone(),
            v["rpc"].as_array().unwrap().iter().map(|rpc| Url::parse(rpc.as_str().unwrap()).unwrap()).collect(),
            v["explorer"].to_string(),
            v["multicall"].as_str().unwrap().parse::<Address>().unwrap_or(Address::ZERO),
            v["currency"].as_str().unwrap().to_owned(),
        ) {
            Ok(n) => networks.push(n),
            Err(e) => tracing::error!("Error creating network {} : {:?}", k, e)
        };
    }
    Ok(networks)
}

pub fn write_to_json_file<T: serde::Serialize>(filename: String, dir_to_create: &str, data: &T) -> Result<()> {
    let data_string = to_string_pretty(data)?;
    // Create results directory if it doesn't exist
    fs::create_dir_all(dir_to_create)?;
    let mut file = fs::File::create(filename)?;
    file.write_all(data_string.as_bytes())?;
    Ok(())
}

pub trait RetryableError {
    fn is_retryable(&self) -> bool;
}

impl RetryableError for ContractError {
    fn is_retryable(&self) -> bool {
        match self {
            ContractError::TransportError(_) => true,
            _ => false,
        }
    }
}

impl<T> RetryableError for RpcError<T> {
    fn is_retryable(&self) -> bool {
        match self {
            RpcError::Transport(_) => true,
            _ => false,
        }
    }
}

pub async fn sleep(duration: time::Duration) {
    tokio::time::sleep(duration).await;
}

pub fn change_rpc(
    wallet: EthereumWallet,
    rpc_urls: &Vec<Url>,
    retry_count: usize,
) -> MyFiller {
    let index = retry_count % rpc_urls.len();
    let rpc_url = rpc_urls[index].clone();
    ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_http(rpc_url)
}

pub async fn retry_async<F, Fut, T, E>(
    f: F,
    max_retries: usize,
    delay: u64
) -> Result<T, E>
where
    F: Fn(usize) -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: RetryableError,
{
    let mut attempt: usize = 0;

    loop {
        match f(attempt).await {
            Ok(value) => return Ok(value),
            Err(e) if attempt < max_retries && e.is_retryable() => {
                attempt += 1;
                sleep(time::Duration::from_millis(delay)).await;
            }
            Err(e) => return Err(e),
        }
    }
}
