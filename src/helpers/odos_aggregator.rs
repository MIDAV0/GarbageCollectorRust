use alloy::{
    network::TransactionBuilder,
    primitives::{utils::parse_units, Address, Bytes, U256},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner
};
use serde_json::Value;
use eyre::Result;
use serde::{Serialize, Deserialize};
use reqwest::Url;

use crate::helpers::web3_client::{Network, Web3Client, GasMultiplier};
use crate::helpers::garbage_collector::TokenData;


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

    pub async fn swap(
        &self,
        token_in: TokenData,
        token_out: TokenData,
        amount_in: U256,
    ) -> Result<()> {
        let quote = self.get_quote(&token_in, &token_out, amount_in).await?;
        self.execute_swap(&token_in, &token_out, quote).await?;
        Ok(())
    }

    async fn get_quote(
        &self,
        token_in: &TokenData,
        token_out: &TokenData,
        amount_in: U256
    ) -> Result<OdosQuoteType> {
        if !SUPPORTED_NETWORKS.contains(&self.network.chain_name.as_str()) {
            return Err(eyre::eyre!(format!("OdosAggregator:get_quote Network {} not supported by Odos", self.network.chain_name)));
        }

        if Self::is_token_native(&token_in.address) && Self::is_token_native(&token_out.address) {
            return Err(eyre::eyre!("OdosAggregator:get_quote Both tokens are native"));
        }

        let payload = OdosQuotePayload {
            chain_id: self.network.id,
            input_tokens: vec![PayloadTokenIn {
                token_address: if Self::is_token_native(&token_in.address) {
                    Address::ZERO
                } else {
                    token_in.address
                },
                amount: amount_in.to_string(),
            }],
            output_tokens: vec![PayloadTokenOut {
                token_address: if Self::is_token_native(&token_out.address) {
                    Address::ZERO
                } else {
                    token_out.address
                },
                proportion: 1,
            }], 
            user_addr: self.signer.address().to_checksum(None),
            slippage_limit_percent: 3.0,
            path_viz: false,
            rederral_code: 1,
            simple: true,
        };

        let client = reqwest::Client::new();
        let res = client.post(self.quote_url.clone())
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;
        if res.status() != 200 {
            match res.text().await {
                Ok(e) => return Err(eyre::eyre!("OdosAggregator:get_quote Failed to get quote from Odos: {}", e)),
                Err(_) => return Err(eyre::eyre!("OdosAggregator:get_quote Failed to fetch quote from Odos")),
            }
        }
        let json: Value = res.json().await?;
        let quote = serde_json::from_value::<OdosQuoteType>(json);
        match quote {
            Ok(q) => Ok(q),
            Err(e) => Err(eyre::eyre!(e)),
        }
    }

    async fn execute_swap(
        &self,
        token_in: &TokenData,
        token_out: &TokenData,
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
            Some(addr) => addr,
            None => return Err(eyre::eyre!("OdosAggregator:execute_swap Could not get approval target")),
        };

        let web3_client = Web3Client::new(self.network.clone(), self.signer.clone())?;
        web3_client.approve(
            token_in.address,
            router_address.parse::<Address>().unwrap(),
            parse_units(quote.in_amounts[0].as_str(), token_in.decimals).unwrap().into(),
            Some(parse_units(quote.in_amounts[0].as_str(), token_in.decimals).unwrap().into())
        ).await?;

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
}

#[tokio::test]
async fn test_get_quote() {
    let signer: PrivateKeySigner = "881406c0472ada6b1c19aee85389a25219ba74d42264bae22dd81ce213a26e91".parse().expect("should parse private key");
    let network = Network {
        id: 8453,
        chain_name: "Base".to_owned(),
        rpc_url: vec!["https://base.publicnode.com".parse::<Url>().unwrap()],
        explorer: "https://basescan.org/tx/".to_owned(),
        multicall: "0xcA11bde05977b3631167028862bE2a173976CA11".parse().unwrap(),
    };
    let odos_aggregator = OdosAggregator::new(signer, network, vec![]).unwrap();
    let token_in = TokenData {
        address: "0x858c50c3af1913b0e849afdb74617388a1a5340d".parse().unwrap(),
        name: "SQT".to_owned(),
        symbol: "SQT".to_owned(),
        decimals: 18,
    };
    let token_out = TokenData {
        address: "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE".parse().unwrap(),
        name: "Ether".to_owned(),
        symbol: "ETH".to_owned(),
        decimals: 18,
    };
    let amount_in = U256::from_str_radix("8ac7230489e80000", 16).unwrap();
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