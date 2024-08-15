use std::collections::HashMap;
use alloy::{primitives::{utils::format_units, Address, U256}, signers::local::PrivateKeySigner};
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
    address: Address,
    name: String,
    symbol: String,
    decimals: u8,
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

    pub fn read_non_zero_balances() -> Result<()> {
        let file_path = "results/nonzero_tokens.json".to_owned();
        let contents = fs::read_to_string(file_path)?;
        let v: HashMap<String, Vec<Balance>> = serde_json::from_str(&contents)?;
        Self::output_report(&v);
        Ok(())
    }

    fn output_report(balances: &HashMap<String, Vec<Balance>>) {
        let mut total_balance: f64 = 0.0;
        for (k, v) in balances.iter() {
            println!("Chain: {}", k);
            let mut total_balance_for_chain: f64 = 0.0;
            for balance in v.iter() {
                if let Ok(converted_balance) = format_units(balance.balance, balance.decimals) {
                    let value = converted_balance.parse::<f64>().unwrap() * balance.token_price;
                    total_balance_for_chain += value;
                    println!("Token: {}, Balance: {}, Value: {}", balance.token_symbol, converted_balance, value);
                } else {
                    println!("Token: {}, Balance: Failed to format balance", balance.token_symbol);
                }
            }
            total_balance += total_balance_for_chain;
            println!("Total balance for chain: {}", total_balance_for_chain);
            println!("---------------------------------\n");
        }
        println!("Total balance: {}", total_balance);
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

    async fn get_token_prices(chain_name: &str, token_balances: &mut [Balance]) -> Result<()> {
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
            let token_address: Address = {
                let temp: Vec<&str> = k.split(':').collect();
                temp[1].parse::<Address>().unwrap()
            };
            let token_balance = token_balances.iter_mut().find(|t_b| t_b.token_address == token_address);
            if let Some(t_b) = token_balance {
                t_b.set_token_price(v["price"].as_f64().unwrap());
                if t_b.token_address == "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE".parse::<Address>().unwrap() {
                    t_b.token_symbol = format!("NATIVE ({})", v["symbol"].as_str().unwrap());
                }
            }
        }
        Ok(())
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
    
                let mut token_datas: Vec<TokenData> = token_list.as_array().unwrap().iter().map(|token| {
                    TokenData {
                        address: token["address"].as_str().unwrap().parse::<Address>().unwrap_or(Address::ZERO),
                        name: token["name"].as_str().unwrap().to_owned(),
                        symbol: token["symbol"].as_str().unwrap().to_owned(),
                        decimals: token["decimals"].as_u64().unwrap() as u8,
                    }
                }).collect();

                // Add native token
                token_datas.push(TokenData {
                    address: "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE".parse::<Address>().unwrap(),
                    name: "NATIVE (ETH)".to_owned(),
                    symbol: "NATIVE (ETH)".to_owned(),
                    decimals: 18,
                });
    
                let res = GarbageCollector::get_non_zero_tokens_for_chain(
                    network,
                    Some("0xBF17a4730Fe4a1ea36Cf536B8473Cc25ba146F19".to_owned()),
                    token_datas,
                    current_signer,
                ).await;
                
                let mut balance_list = match res {
                    Ok(b_l) => b_l,
                    Err(e) => {
                        println!("Error getting balance list: {:?}", e);
                        return;
                    }
                };
                if !balance_list.is_empty() {
                    GarbageCollector::get_token_prices(&k, &mut balance_list).await.unwrap();
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
        // Output report
        Self::output_report(&final_result);
        Self::write_to_json_file("results/nonzero_tokens.json".to_owned(), "results", &*final_result)?;
        Ok(())
    }

    async fn get_non_zero_tokens_for_chain(
        network: web3_client::Network,
        target_wallet_: Option<String>,
        token_datas: Vec<TokenData>,
        signer: PrivateKeySigner,
    ) -> Result<Vec<Balance>> {
        let target_wallet = match target_wallet_ {
            Some(t_w) => t_w.parse::<Address>().unwrap(),
            None => signer.address(),
        };

        let web3_client = web3_client::Web3Client::new(network, signer).unwrap();
        
        let balance_list =  match web3_client.call_balance(target_wallet, token_datas).await {
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
    let _ = GarbageCollector::read_non_zero_balances().unwrap();
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

#[tokio::test]
async fn test_get_native_token_price() -> Result<()> {
    let mut balance = Balance::new(
        "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE".parse::<Address>().unwrap(),
        "NATIVE (ETH)".to_owned(),
        "NATIVE (ETH)".to_owned(),
        18,
        U256::from(1000000000),
    );
    let mut balances = vec![balance];
    let _ = GarbageCollector::get_token_prices("Ethereum", &mut balances).await?;
    println!("{:?}", balances[0].token_price);
    Ok(())
}