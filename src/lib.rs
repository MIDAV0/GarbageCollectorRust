use std::collections::HashMap;
use alloy::{dyn_abi::abi::token, primitives::{utils::format_ether, Address, U256}, signers::local::PrivateKeySigner, uint};
use const_types::ChainName;
use web3_client::Balance;
use std::fs;
use serde_json::{to_string_pretty, Value};
use eyre::Result;
use reqwest::Url;
use std::io::Write;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task;

mod web3_client;
mod const_types;

struct TokenData {
    chain_id: u32,
    address: Address,
    name: String,
    symbol: String,
    decimals: u8,
    logo_uri: String,
}

// struct NonzeroTokenData {
//     address: String,
//     name: String,
//     symbol: String,
//     decimals: u8,
//     balance: u128,
// }

pub struct GarbageCollector {
    signer: PrivateKeySigner,
    // Chain JSON data
    chain_data: Value,
}

impl Default for GarbageCollector {
    fn default() -> Self {
        GarbageCollector {
            signer: PrivateKeySigner::random(),
            chain_data: Value::Null,
        }
    }
}

impl GarbageCollector {
    pub fn new() -> Self {
        let chain_data = GarbageCollector::parse_json_data("data/chains.json".to_owned()).unwrap();
        GarbageCollector {
            chain_data,
            ..Default::default()
        }
    }

    // Connect signer to the garbage collector
    pub fn connect_signer(&mut self, signer_: PrivateKeySigner) {
        self.signer = signer_;
    }

    pub fn read_non_zero_balances(&self) -> Result<()> {
        let file_path = "results/nonzero_tokens.json".to_owned();
        let contents = fs::read_to_string(file_path)?;
        let v: HashMap<String, Vec<Balance>> = serde_json::from_str(&contents)?;
        for (k, v) in v.iter() {
            println!("Chain: {}", k);
            for balance in v.iter() {
                println!("Token: {}, Balance: {}", balance.token_address, format_ether(balance.balance));
            }
        }
        Ok(())
    }

    // Parse JSON file
    fn parse_json_data(file_path: String) -> Result<Value> {
        let contents = fs::read_to_string(file_path)?;
        let v: Value = serde_json::from_str(&contents)?;
        Ok(v)
    }

    fn write_to_json_file<T: serde::Serialize>(filename: String, dir_to_create: &str, data: &T) -> Result<()> {
        let data_string = to_string_pretty(data)?;
        // Create results directory if it doesn't exist
        fs::create_dir_all(dir_to_create)?;
        let mut file = fs::File::create(filename)?;
        file.write_all(data_string.as_bytes())?;
        Ok(())
    }

    async fn fetch_token_data(chain_name: &String) -> Result<Value> {
        let url = format!("https://tokens.coingecko.com/{}/all.json", const_types::convert_network_name_to_coingecko_query_string(ChainName::from(chain_name.as_str())));
        let url = Url::parse(&url)?;
        let res = reqwest::get(url).await?;
        let json: Value = res.json().await?;
        let token_data = &json["tokens"];
        if token_data.is_null() {
            return Err(eyre::eyre!("Token data is null"));
        }
        Self::write_to_json_file(format!("data/token_lists/{}.json", chain_name), "data/token_lists", token_data)?;
        fs::create_dir_all("data/token_lists")?;
        Ok(json)
    }

    async fn get_token_prices(chain_name: &str, mut token_balances: &Vec<Balance>) -> Result<Value> {
        let chain = match chain_name {
            "Zksync" => "era".to_owned(),
            "Nova" => "arbitrum_nova".to_owned(),
            v => v.to_owned(),
        };

        let mut url = "https://coins.llama.fi/prices/current/".to_owned();
        token_balances.iter().enumerate().for_each(|(i, token_balance)| {
            let token_address = token_balance.token_address;
            url.push_str(
                format!(
                    "{}:{}{}",
                    chain,
                    token_address,
                    if i + 1 == token_balances.len() { "" } else { "," },
                ).as_str()
            );
        });

        let url = Url::parse(&url)?;
        let res = reqwest::get(url).await?;
        let json: Value = res.json().await?;
        let coins = &json["coins"];
        if coins.is_null() {
            return Err(eyre::eyre!("Coins data is null"));
        }

        for (k, v) in coins.as_object().unwrap() {
            let price = v["price"].as_f64().unwrap();
            let symbol = v["symbol"].as_str().unwrap();
            let confidence = v["confidence"].as_f64().unwrap();
            let timestamp = v["timestamp"].as_i64().unwrap();
            println!("Token: {}, Price: {}, Symbol: {}, Confidence: {}, Timestamp: {}", k, price, symbol, confidence, timestamp);
        }

        // {
        //     "coins": {
        //       "ethereum:0x6ff2241756549b5816a177659e766eaf14b34429": {
        //         "decimals": 18,
        //         "price": 0.00240279962047993,
        //         "symbol": "AQTIS",
        //         "timestamp": 1723410561,
        //         "confidence": 0.98
        //       }
        //     }
        //   }

        Ok(Value::Null)
    }

    async fn get_token_data(chain_name: &String) -> Result<Value> {
        match Self::parse_json_data(format!("data/token_lists/{}.json", chain_name)) {
            Ok(v) => Ok(v),
            Err(_) => {
                println!("Fetching token data from Coingecko API");
                Self::fetch_token_data(chain_name).await
            }
        }
    }

    pub async fn get_non_zero_tokens(&self) -> Result<()> {

        let results = Arc::new(Mutex::new(HashMap::<String, Vec<Balance>>::new()));
        let mut handles = vec![];
        let chain_data = {
            let cloned_chain_data = self.chain_data.clone();
            cloned_chain_data.as_object().unwrap().clone()
        };

        for (k, v) in chain_data {
            let results_clone = Arc::clone(&results);
            let network = match web3_client::Network::new( 
                v["id"].as_i64().unwrap() as i32,
                v["lz_id"].to_string(),
                v["rpc"].as_array().unwrap().iter().map(|rpc| Url::parse(rpc.as_str().unwrap()).unwrap()).collect(),
                v["explorer"].to_string(),
                v["multicall"].as_str().unwrap().parse::<Address>().unwrap_or(Address::ZERO),
            ) {
                Ok(n) => n,
                Err(e) => {
                    println!("Error creating network {} : {:?}", k, e);
                    continue;
                }
            };
            let current_signer = self.signer.clone();
            let handle = task::spawn(async move {
                let token_list_result = Self::get_token_data(&k).await;
                let token_list = match token_list_result {
                    Ok(t_l) =>t_l,
                    Err(_) => return,
                };
    
                let token_addresses: Vec<Address> = token_list.as_array().unwrap().iter().map(|token| {
                    token["address"].as_str().unwrap().parse::<Address>().unwrap_or(Address::ZERO)
                }).collect();
    
                let res = GarbageCollector::get_non_zero_tokens_for_chain(
                    network,
                    Some("0xBF17a4730Fe4a1ea36Cf536B8473Cc25ba146F19".to_owned()),
                    token_addresses,
                    current_signer,
                ).await;
                
                let balance_list = match res {
                    Ok(b_l) => b_l,
                    Err(e) => {
                        println!("Error getting balance list: {:?}", e);
                        return;
                    }
                };
                if !balance_list.is_empty() {
                    let mut results = results_clone.lock().await;
                    results.insert(k.to_string(), balance_list);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let final_result = results.lock().await;

        Self::write_to_json_file("results/nonzero_tokens.json".to_owned(), "results", &*final_result)?;
        Ok(())
    }

    async fn get_non_zero_tokens_for_chain(
        network: web3_client::Network,
        target_wallet_: Option<String>,
        token_addresses: Vec<Address>,
        signer: PrivateKeySigner,
    ) -> Result<Vec<Balance>> {
        let target_wallet = match target_wallet_ {
            Some(t_w) => t_w.parse::<Address>().unwrap(),
            None => signer.address(),
        };

        let web3_client = web3_client::Web3Client::new(network, signer).unwrap();
        
        let balance_list =  match web3_client.call_balance(target_wallet, token_addresses).await {
            Ok(b_l) => b_l,
            Err(e) => {
                println!("Error getting balance list: {:?}", e);
                return Err(eyre::eyre!("Error getting balance list"));
            }
        };
        Ok(balance_list)
    }
}


#[test]
fn test_json_parser() {
    let result = GarbageCollector::parse_json_data("data/chains.json".to_owned());
    assert_eq!(result.is_ok(), true);
}

#[test]
fn test_read_non_zero_balances() {
    let garbage_collector = GarbageCollector::new();
    let _ = garbage_collector.read_non_zero_balances().unwrap();
}

#[tokio::test]
async fn test_get_non_zero_tokens() {
    let mut garbage_collector = GarbageCollector::new();
    let _ = garbage_collector.get_non_zero_tokens().await.unwrap();
}

#[tokio::test]
async fn test_token_fetch() -> Result<()> {
    let tn = "Manta".to_owned();
    let d = GarbageCollector::get_token_data(&tn).await?;
    println!("{:?}", d);
    Ok(())
}

#[test]
fn parse_token() {
    let balance = uint!(0x18d192fd096b80bbdd8_U256);
    let converted = format_ether(balance);
    println!("{}", converted);
}