use std::collections::HashMap;
use alloy::{primitives::{utils::format_ether, Address, U256}, signers::local::PrivateKeySigner};
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

    fn write_to_json_file(filename: &str, data: &HashMap<String, Vec<Balance>>) -> Result<()> {
        // Create results directory if it doesn't exist
        fs::create_dir_all("results")?;
        let filename = format!("results/{}", filename);
        let json = to_string_pretty(&data)?;
        let mut file = fs::File::create(filename)?;
        file.write_all(json.as_bytes())?;
        Ok(())
    }

    pub async fn get_non_zero_tokens(&mut self) -> Result<()> {

        let results = Arc::new(Mutex::new(HashMap::<String, Vec<Balance>>::new()));
        let mut handles = vec![];
        let c_d = {
            let cloned_chain_data = self.chain_data.clone();
            cloned_chain_data.as_object().unwrap().clone()
        };

        for (k, _) in c_d {
            let results_clone = Arc::clone(&results);
            let network = web3_client::Network::new( 
                self.chain_data[&k]["id"].as_i64().unwrap() as i32,
                self.chain_data[&k]["lz_id"].to_string(),
                self.chain_data[&k]["rpc"].as_array().unwrap().iter().map(|rpc| Url::parse(rpc.as_str().unwrap()).unwrap()).collect(),
                self.chain_data[&k]["explorer"].to_string(),
                self.chain_data[&k]["multicall"].as_str().unwrap().parse::<Address>().unwrap_or(Address::ZERO),
            );
            let current_signer = self.signer.clone();
            let handle = task::spawn(async move {
                let token_list_result = GarbageCollector::parse_json_data(format!("data/token_lists/{}.json", k));
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

        Self::write_to_json_file("nonzero_tokens.json", &final_result)?;
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