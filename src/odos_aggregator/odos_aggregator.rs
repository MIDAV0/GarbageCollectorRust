use std::sync::Arc;

use alloy::{
    primitives::{utils::parse_units, Address, Bytes, U256},
    providers::ProviderBuilder,
};
use serde_json::Value;
use eyre::Result;
use serde::{Serialize, Deserialize};
use reqwest::{header::HeaderMap, Method};
use tokio::task::JoinSet;

use crate::{db::{account::Account, database::Database}, helpers::{fetch::{send_http_request_with_retries, RequestParams}, utils::{get_networks, get_user_tokens_from_file}}, web3_client::web3_client::{GasMultiplier, Web3Client}};
use crate::constants::{TokenData, Network};

use super::constants::ODOS_API_URL;


static SUPPORTED_NETWORKS: [&str; 12] = [
    "Ethereum",
    "Arbitrum",
    "Avalanche",
    "Polygon",
    "Bsc",
    "Optimism",
    "Base",
    "Fantom",
    "Zksync",
    "Linea",
    "Scroll",
    "Mantle"
];

#[derive(Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
struct PayloadTokenIn {
    amount: String,
    token_address: Address,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
struct PayloadTokenOut {
    proportion: u8,
    token_address: Address,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
struct OdosQuotePayload {
    chain_id: u32,
    input_tokens: Vec<PayloadTokenIn>,
    output_tokens: Vec<PayloadTokenOut>,
    user_addr: String,
    slippage_limit_percent: f64,
    path_viz: bool,
    rederral_code: u8,
    simple: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
struct OdosRouterConfig {
    chain_id: u32,
    router_address: String,
    executor_address: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
struct OdosAssemblePayload {
    user_addr: String,
    path_id: String,
    simulate: bool,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all="camelCase")]
struct OdosQuoteType {
    block_number: u64,
    data_gas_estimate: u64,
    gas_estimate: f64,
    gas_estimate_value: f64,
    gwei_per_gas: f64,
    in_amounts: Vec<String>,
    in_tokens: Vec<String>,
    in_values: Vec<f64>,
    net_out_value: f64,
    out_amounts: Vec<String>,
    out_tokens: Vec<String>,
    out_values: Vec<f64>,
    partner_fee_percent: f64,
    path_id: String,
    path_viz: Option<String>,
    percent_diff: f64,
    price_impact: f64,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all="camelCase")]
struct OdosAssembleType {
    gas: Option<u128>,
    gas_price: u128,
    value: String,
    to: Address,
    from: Address,
    data: Bytes,
    nonce: u64,
    chain_id: u64,
}

pub async fn execute_odos_swap(db: Database) -> Result<()> {
    let accounts: Vec<&Account> = db.0.iter()
            .map(|account| account)
            .collect();

    // Get chain data in Arc
    let chain_data = Arc::new(get_networks()?);

    for account in accounts {
        if let Err(e) = swap(
            account,
            Arc::clone(&chain_data)
        ).await {
            tracing::error!("Error swapping");
        };
    }

    Ok(())
}

pub async fn swap(
    account: &Account,
    chain_data: Arc<Vec<Network>>,
) -> Result<()> {

    let balances = get_user_tokens_from_file(account.get_address().to_string())?;

    let mut handles = JoinSet::new();

    for network in chain_data.iter() {
        let network = network.clone();
        let chain_balances = match balances.get(&network.chain_name) {
            Some(balance) => {
                if balance.is_empty() {
                    continue;
                }
                balance
            },
            None => continue,
        };

        let account = account.clone();
        handles.spawn(async move {
            let mut token_bundle = Vec::<TokenData>::with_capacity(5);
            let mut amount_bundle = Vec::<U256>::with_capacity(5);

            for (index, chain_balance) in chain_balances.iter().enumerate() {
    
                let token = TokenData {
                    address: chain_balance.token_address,
                    name: chain_balance.token_name.clone(),
                    symbol: chain_balance.token_symbol.clone(),
                    decimals: chain_balance.decimals,
                };

                token_bundle.push(token);
                amount_bundle.push(chain_balance.balance);

                if token_bundle.len() == 5 || index == chain_balances.len() - 1 {

                    get_quote(
                        &account,
                        &token_bundle,
                        &amount_bundle,
                        &network,
                    ).await;

                    token_bundle.clear();
                    amount_bundle.clear();   
                }
            }
        });
    }
    Ok(())
}

async fn get_quote(
    account: &Account,
    tokens_in: &Vec<TokenData>,
    amounts_in: &Vec<U256>,
    network: &Network,
) -> Result<()> {
    if !SUPPORTED_NETWORKS.contains(&network.chain_name.as_str()) {
        tracing::error!("OdosAggregator:get_quote Network {} not supported by Odos", network.chain_name);
        return Err(eyre::eyre!(format!("OdosAggregator:get_quote Network {} not supported by Odos", network.chain_name)));
    }

    if tokens_in.iter().any(|t| is_token_native(&t.address)) {
        return Err(eyre::eyre!("OdosAggregator:get_quote Trying to swap from native token to native token"));
    }

    let payload = OdosQuotePayload {
        chain_id: network.id,
        input_tokens: tokens_in.iter().zip(amounts_in.iter()).map(|(t, a)| PayloadTokenIn {
            token_address: t.address,
            amount: a.to_string(),
        }).collect(),
        output_tokens: vec![PayloadTokenOut {
            token_address: Address::ZERO,
            proportion: 1,
        }], 
        user_addr: account.get_address().to_checksum(None),
        slippage_limit_percent: 3.0,
        path_viz: false,
        rederral_code: 1,
        simple: true,
    };

    let method = Method::POST;
    let path = "/sor/quote/v2";
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());

    let request_params = RequestParams {
        url: &format!("{}{}", ODOS_API_URL, path),
        method,
        body: Some(payload),
        query_args: None,
    };

    let response = send_http_request_with_retries::<OdosQuoteType>(
        &request_params,
        Some(&headers),
        None,
        None,
        None,
        |_| true,
    ).await?;

    let quote = match response.body {
        Some(q) => Ok(q),
        None => Err(eyre::eyre!("OdosAggregator:get_quote Failed to get quote")),
    };

    execute_swap(account,
        &tokens_in,
        quote.unwrap(),
        network
    ).await
}

async fn execute_swap(
    account: &Account,
    tokens_in: &Vec<TokenData>,
    quote: OdosQuoteType,
    network: &Network,
) -> Result<()> {
    let method = Method::GET;
    let path = format!("/info/contract-info/v2/{}", network.id);
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());

    let request_params = RequestParams {
        url: &format!("{}{}", ODOS_API_URL, path),
        method,
        body: None::<serde_json::Value>,
        query_args: None,
    };

    let response = send_http_request_with_retries::<OdosRouterConfig>(
        &request_params,
        Some(&headers),
        None,
        None,
        None,
        |_| true,
    ).await?;

    let router_address = match response.body {
        Some(q) => q.router_address.parse::<Address>().unwrap(),
        None => return Err(eyre::eyre!("OdosAggregator:get_quote Failed to get quote")),
    };

    let provider = Arc::new(
        ProviderBuilder::new()
            .with_recommended_fillers()
            .on_http(network.rpc_url[0].clone()),
    );

    let mut web3_client = Web3Client::new(
        provider,
        Some(account.get_private_key()),
        network.clone()
    )?;

    // Approve tokens
    for (i, token) in tokens_in.iter().enumerate() {
        match web3_client.approve(
            token.address,
            router_address,
            parse_units(quote.in_amounts[i].as_str(), token.decimals).unwrap().into(),
            Some(parse_units(quote.in_amounts[i].as_str(), token.decimals).unwrap().into())
        ).await {
            Ok(_) => (),
            Err(_) => (),
        }
    }

    let payload = OdosAssemblePayload {
        user_addr: account.get_address().to_checksum(None),
        path_id: quote.path_id,
        simulate: true,
    };

    let method = Method::POST;
    let path = "/info/contract-info/v2";
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());

    let request_params = RequestParams {
        url: &format!("{}{}", ODOS_API_URL, path),
        method,
        body: Some(payload),
        query_args: None,
    };

    let response = send_http_request_with_retries::<Value>(
        &request_params,
        Some(&headers),
        None,
        None,
        None,
        |_| true,
    ).await?;

    let response_data = match response.body {
        Some(q) => q,
        None => return Err(eyre::eyre!("OdosAggregator:get_quote Failed to get quote")),
    };

    if !response_data["simulation"]["isSuccess"].as_bool().unwrap() {
        return Err(eyre::eyre!("OdosAggregator:execute_swap Failed to simulate swap"));
    };
    let tx = match serde_json::from_value::<OdosAssembleType>(response_data["transaction"].clone()) {
        Ok(q) => q,
        Err(e) => return Err(eyre::eyre!(e)),
    };

    let gas_price_multiplier: f32 = if network.chain_name == "Ethereum" || network.chain_name == "Polygon" || network.chain_name == "Avalanche" {1.1} else {1.0};

    let _ = web3_client.send_tx(
        tx.to,
        Some(tx.data),
        Some(GasMultiplier::new(gas_price_multiplier, 1.1))).await?;

    Ok(())
}

fn is_token_native(token_address: &Address) -> bool {
    *token_address == Address::ZERO ||
        *token_address == "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE".parse::<Address>().unwrap() ||
        *token_address == "0x0000000000000000000000000000000000001010".parse::<Address>().unwrap()
}


#[tokio::test]
async fn test_get_quote() {
    let signer: PrivateKeySigner = "8dcad20d2291af38f28fb6a0cd83e270607106008a298bc05fbcc137b8b47f96".parse().expect("should parse private key");
    let network = Network {
        id: 8453,
        chain_name: "Base".to_owned(),
        rpc_url: vec!["https://base.publicnode.co".parse::<Url>().unwrap(), "https://base.publicnode.com".parse::<Url>().unwrap()],
        explorer: "https://basescan.org/tx/".to_owned(),
        multicall: "0xcA11bde05977b3631167028862bE2a173976CA11".parse().unwrap(),
    };
    let mut web3_client = Web3Client::new(network.clone(), signer.clone()).unwrap();

    let token_in = TokenData {
        address: "0x858c50c3af1913b0e849afdb74617388a1a5340d".parse().unwrap(),
        name: "SQT".to_owned(),
        symbol: "SQT".to_owned(),
        decimals: 18,
    };

    let d = web3_client.approve(
        token_in.address,
        "0xBF17a4730Fe4a1ea36Cf536B8473Cc25ba146F19".parse().unwrap(),
        parse_units("1000000000000000000", token_in.decimals).unwrap().into(),
        Some(parse_units("1000000000000000000", token_in.decimals).unwrap().into())
    ).await;

    match d {
        Ok(q) => println!("{:?}", q),
        Err(e) => println!("{:?}", e),
    }
}

#[tokio::test]
async fn test_swap() {
    let signer: PrivateKeySigner = "".parse().expect("should parse private key");
    let network = Network {
        id: 8453,
        chain_name: "Base".to_owned(),
        rpc_url: vec!["https://base.publicnode.com".parse::<Url>().unwrap()],
        explorer: "https://basescan.org/tx/".to_owned(),
        multicall: "0xcA11bde05977b3631167028862bE2a173976CA11".parse().unwrap(),
    };
    let odos_aggregator = OdosAggregator::new(signer, network, vec![]).unwrap();
    let token_in = vec![TokenData {
        address: "0x858c50c3af1913b0e849afdb74617388a1a5340d".parse().unwrap(),
        name: "SQT".to_owned(),
        symbol: "SQT".to_owned(),
        decimals: 18,
    }];
    let token_out = vec![TokenData {
        address: "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE".parse().unwrap(),
        name: "Ether".to_owned(),
        symbol: "ETH".to_owned(),
        decimals: 18,
    }];
    let amount_in = vec![U256::from_str_radix("8ac7230489e80000", 16).unwrap()];
    let d = odos_aggregator.swap(token_in, token_out, amount_in).await;
    match d {
        Ok(q) => println!("{:?}", q),
        Err(e) => println!("{:?}", e),
    }
}

#[test]
fn test_bigint() -> Result<()> {
    // Convert 0x75899e7357ec6f0e00000 to U256
    let amount = U256::from_str_radix("8ac7230489e80000", 16)?;
    println!("{:?}", amount);
    Ok(())
}