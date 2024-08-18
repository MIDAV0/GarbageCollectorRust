use alloy::{primitives::{Address, U256},signers::local::PrivateKeySigner};
use serde_json::Value;
use crate::web3_client::Network;
use crate::TokenData;
use eyre::Result;
use serde::{Serialize, Deserialize};
use reqwest::Url;

const SUPPORTED_NETWORKS: [&str; 12] = [
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
    user_addr: Address,
    slippage_limit_percent: f64,
    path_viz: bool,
    rederral_code: u8,
    simple: bool,
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
    ) -> Result<bool> {
        let quote = self.get_quote(&token_in, &token_out, amount_in).await?;
        self.execute_swap(&token_in, &token_out, quote).await?;
        Ok(true)
    }

    async fn get_quote(
        &self,
        token_in: &TokenData,
        token_out: &TokenData,
        amount_in: U256
    ) -> Result<OdosQuoteType> {
        if !SUPPORTED_NETWORKS.contains(&self.network.chain_name.as_str()) {
            return Err(eyre::eyre!(format!("Network {} not supported by Odos", self.network.chain_name)));
        }

        if Self::is_token_native(&token_in.address) && Self::is_token_native(&token_out.address) {
            return Err(eyre::eyre!("Both tokens are native"));
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
                token_address: token_out.address,
                proportion: 1,
            }],
            user_addr: self.signer.address(),
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
            return Err(eyre::eyre!("Failed to get quote"));
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
    let signer = PrivateKeySigner::random();
    let network = Network {
        id: 1,
        chain_name: "Ethereum".to_owned(),
        rpc_url: vec!["https://ethereum.publicnode.com".parse::<Url>().unwrap()],
        explorer: "https://etherscan.io/tx/".to_owned(),
        multicall: "0xcA11bde05977b3631167028862bE2a173976CA11".parse().unwrap(),
    };
    let odos_aggregator = OdosAggregator::new(signer, network, vec![]).unwrap();
    let token_in = TokenData {
        address: "0x6ff2241756549b5816a177659e766eaf14b34429".parse().unwrap(),
        name: "AQTIS".to_owned(),
        symbol: "AQTIS".to_owned(),
        decimals: 18,
    };
    let token_out = TokenData {
        address: "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2".parse().unwrap(),
        name: "Wrapped Ether".to_owned(),
        symbol: "WETH".to_owned(),
        decimals: 18,
    };
    let amount_in = U256::from(1000000000000000_i64);
    odos_aggregator.get_quote(&token_in, &token_out, amount_in).await.unwrap();
}

#[test]
fn test_bigint() {
    // Convert 0xd14c4827a2cd7a62 to U256
    let value = U256::from(0xd14c4827a2cd7a62_u64);
    let d = value.to_string();
    println!("{:?}", d);
}