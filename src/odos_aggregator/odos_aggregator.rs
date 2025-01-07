use alloy::{
    network::TransactionBuilder,
    primitives::{utils::parse_units, Address, Bytes, U256},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner
};
use serde_json::Value;
use eyre::Result;
use serde::{Serialize, Deserialize};
use reqwest::{header::HeaderMap, Method, Url};

use crate::{helpers::fetch::{send_http_request_with_retries, RequestParams}, web3_client::web3_client::{GasMultiplier, Web3Client}};
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

pub struct OdosAggregator {
    signer: PrivateKeySigner,
    network: Network,
    proxies: Vec<String>,
    quote_url: Url,
    assemble_url: Url,
}

impl OdosAggregator {
    pub fn new(
        signer: PrivateKeySigner,
        network: Network,
        proxies: Vec<String>,
    ) -> Result<Self> {
        Ok(OdosAggregator {
            signer,
            network,
            proxies,
            quote_url: Url::parse("https://api.odos.xyz/sor/quote/v2")?,
            assemble_url: Url::parse("https://api.odos.xyz/sor/assemble")?,
        })
    }
}

pub async fn swap(
    tokens_in: Vec<TokenData>,
    tokens_out: Vec<TokenData>,
    amounts_in: Vec<U256>,
    network: Network,
) -> Result<()> {
    let quote = get_quote(
        &tokens_in,
        &tokens_out,
        amounts_in,
        &network,
    ).await?;
    execute_swap(&tokens_in, quote).await?;
    Ok(())
}

async fn get_quote(
    tokens_in: &Vec<TokenData>,
    tokens_out: &Vec<TokenData>,
    amounts_in: Vec<U256>,
    network: &Network,
) -> Result<OdosQuoteType> {
    if !SUPPORTED_NETWORKS.contains(&network.chain_name.as_str()) {
        tracing::error!("OdosAggregator:get_quote Network {} not supported by Odos", network.chain_name);
        return Err(eyre::eyre!(format!("OdosAggregator:get_quote Network {} not supported by Odos", network.chain_name)));
    }

    if 
        tokens_in.iter().any(|t| is_token_native(&t.address))
        && is_token_native(&tokens_out[0].address) 
    {
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
        user_addr: self.signer.address().to_checksum(None),
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

    match response.body {
        Some(q) => Ok(q),
        None => Err(eyre::eyre!("OdosAggregator:get_quote Failed to get quote")),
    }
}

async fn execute_swap(
    &self,
    tokens_in: &Vec<TokenData>,
    quote: OdosQuoteType,
) -> Result<()> {
    let url_str = format!("https://api.odos.xyz/info/contract-info/v2/{}", self.network.id);
    let url = Url::parse(&url_str)?;
    let client = reqwest::Client::new();
    let res = client.get(url)
        .header("Content-Type", "application/json")
        .send()
        .await?;
    if res.status() != 200 {
        return Err(eyre::eyre!("OdosAggregator:execute_swap Failed to get contract info"));
    }
    let json: Value = res.json().await?;
    let router_address = match json["routerAddress"].as_str() {
        Some(addr) => addr.parse::<Address>().unwrap(),
        None => return Err(eyre::eyre!("OdosAggregator:execute_swap Could not get approval target")),
    };

    let mut web3_client = Web3Client::new(provider, pk, self.network.clone())?;

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
        user_addr: self.signer.address().to_checksum(None),
        path_id: quote.path_id,
        simulate: true,
    };

    let res = client.post(self.assemble_url.clone())
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;
    if res.status() != 200 {
        return Err(eyre::eyre!("OdosAggregator:execute_swap Failed to assempble swap"));
    }

    let json: Value = res.json().await?;
    if !json["simulation"]["isSuccess"].as_bool().unwrap() {
        return Err(eyre::eyre!("OdosAggregator:execute_swap Failed to simulate swap"));
    };
    let tx = match serde_json::from_value::<OdosAssembleType>(json["transaction"].clone()) {
        Ok(q) => q,
        Err(e) => return Err(eyre::eyre!(e)),
    };

    let adjusted_tx = TransactionRequest::default()
        .with_from(tx.from)
        .with_to(tx.to)
        .with_nonce(tx.nonce)
        .with_input(tx.data)
        .with_gas_price(tx.gas_price)
        .with_gas_limit(tx.gas.unwrap_or(0));

    let gas_price_multiplier: f32 = if self.network.chain_name == "Ethereum" || self.network.chain_name == "Polygon" || self.network.chain_name == "Avalanche" {1.1} else {1.0};

    let _ = web3_client.send_tx(adjusted_tx, Some(GasMultiplier::new(gas_price_multiplier, 1.1))).await?;

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