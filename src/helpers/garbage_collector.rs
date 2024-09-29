use alloy::{primitives::{utils::format_units, Address, U256}, signers::local::PrivateKeySigner};
use const_types::ChainName;
use serde_json::{to_string_pretty, Value};
use eyre::Result;
use reqwest::Url;
use std::{io::Write, sync::Arc, fs, collections::HashMap};
use tokio::{task, sync::Mutex};
use log::{info, error};

use crate::{constants::const_types::Env, helpers::web3_client::*};
use crate::constants::const_types;

pub struct TokenData {
    pub address: Address,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
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
    debug: bool,
}

impl Default for GarbageCollector {
    fn default() -> Self {
        GarbageCollector {
            signer: PrivateKeySigner::random(),
            chain_data: Value::Null,
            debug: false,
        }
    }
}

impl GarbageCollector {
    pub fn new() -> Self {
        let chain_data = GarbageCollector::parse_json_data("data/chains.json".to_owned()).unwrap();
        let env = Env::new();
        GarbageCollector {
            chain_data,
            debug: env.debug,
            ..Default::default()
        }
    }

    // Connect signer to the garbage collector
    pub fn connect_signer(&mut self, signer_: PrivateKeySigner) {
        self.signer = signer_;
    }

    pub fn read_non_zero_balances(target_address: String) -> Result<()> {
        let file_path = format!("results/tokens_{}.json", target_address.to_lowercase());
        let contents = match fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => return Err(eyre::eyre!("Failed to read file")),
        };
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
        let token_data = match json.get("tokens") {
            Some(t_d) => t_d,
            None => return Err(eyre::eyre!("Token data is null")),
        };
        Self::write_to_json_file(format!("data/token_lists/{}.json", chain_name), "data/token_lists", token_data)?;
        fs::create_dir_all("data/token_lists")?;
        Ok(token_data.clone())
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

    pub async fn get_non_zero_tokens(&self, target_address_: Option<String>) -> Result<()> {

        let results = Arc::new(Mutex::new(HashMap::<String, Vec<Balance>>::new()));
        let mut handles = vec![];
        let chain_data = {
            let cloned_chain_data = self.chain_data.clone();
            cloned_chain_data.as_object().unwrap().clone()
        };

        let target_address = match &target_address_ {
            Some(t_a) => t_a.parse::<Address>().unwrap(),
            None => self.signer.address(),
        };

        for (k, v) in chain_data {
            let results_clone = Arc::clone(&results);
            let network = match Network::new( 
                v["id"].as_u64().unwrap() as u32,
                k.clone(),
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
                    name: v["currency"].as_str().unwrap().to_owned(),
                    symbol: v["currency"].as_str().unwrap().to_owned(),
                    decimals: 18,
                });
    
                let res = GarbageCollector::get_non_zero_tokens_for_chain(
                    network,
                    target_address,
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
        Self::write_to_json_file(format!("results/tokens_{}.json", target_address), "results", &*final_result)?;
        Ok(())
    }

    async fn get_non_zero_tokens_for_chain(
        network: Network,
        target_wallet: Address,
        token_datas: Vec<TokenData>,
        signer: PrivateKeySigner,
    ) -> Result<Vec<Balance>> {

        let web3_client = Web3Client::new(network, signer).unwrap();
        
        let balance_list =  match web3_client.call_balance(target_wallet, token_datas).await {
            Ok(b_l) => b_l,
            Err(e) => {
                println!("Error getting balance list: {:?}", e);
                return Err(eyre::eyre!("Error getting balance list"));
            }
        };
        Ok(balance_list)
    }

    async fn swap_tokens_to_native_for_chain(
        network: Network,
        token_balances: &Vec<Balance>
    ) -> Result<()> {
        // Shuffle tokens option
        // let native_token_data: Balance = Balance::new(
        //     "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE".parse::<Address>().unwrap(),
            
        //     "NATIVE (ETH)".to_owned(),
        //     18,
        //     U256::from(0),
        // );

        Ok(())
    }
}


#[test]
fn test_json_parser() {
    let result = GarbageCollector::parse_json_data("data/chains.json".to_owned());
    assert_eq!(result.is_ok(), true);
}

#[test]
fn test_read_non_zero_balances() {
    let _ = GarbageCollector::read_non_zero_balances("0xBF17a4730Fe4a1ea36Cf536B8473Cc25ba146F19".to_owned()).unwrap();
}

#[tokio::test]
async fn test_get_non_zero_tokens() {
    let garbage_collector = GarbageCollector::new();
    let _ = garbage_collector.get_non_zero_tokens(Some("0xf63feA8d383b8089BAbFf2A712AB3190CB21732D".to_owned())).await.unwrap();
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
    let balance = Balance::new(
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