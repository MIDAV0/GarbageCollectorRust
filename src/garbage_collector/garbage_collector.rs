use alloy::primitives::{utils::format_units, Address};
use alloy::providers::ProviderBuilder;
use serde_json::Value;
use eyre::Result;
use reqwest::Url;
use std::{sync::Arc, fs, collections::HashMap};
use tokio::{task::JoinSet, sync::Mutex};

use crate::web3_client::web3_client::{Web3Client, Balance};
use crate::constants::{TokenData, ChainName, Network, convert_network_name_to_coingecko_query_string};
use crate::db::database::Database;
use crate::helpers::utils::{get_networks, get_user_tokens_from_file, parse_json_data, write_to_json_file};


pub async fn get_balances(db: Database) -> Result<()> {
    let addresses: Vec<Address> = db.0.iter()
            .map(|account| account.get_address())
            .collect();

    // Get chain data in Arc
    let chain_data = Arc::new(get_networks()?);

    for address in addresses {
        if let Err(e) =
         get_non_zero_tokens(address, Arc::clone(&chain_data)).await {
            tracing::error!("Error getting non zero tokens for address {} : {:?}", address, e);
        };
    }

    Ok(())
}

pub async fn get_non_zero_tokens(target_address: Address, chain_data: Arc<Vec<Network>>) -> Result<()> {

    let results = Arc::new(Mutex::new(HashMap::<String, Vec<Balance>>::new()));
    let mut handles = JoinSet::new();

    for network in chain_data.iter() {
        let network = network.clone();

        let results_clone = Arc::clone(&results);

        handles.spawn(async move {
            let mut token_datas = match get_token_data(&network.chain_name).await {
                Ok(t_d) => t_d,
                Err(_) => return
            };

            // Add native token
            token_datas.push(TokenData {
                address: "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE".parse::<Address>().unwrap(),
                name: network.currency.clone(),
                symbol: network.currency.clone(),
                decimals: 18,
            });

            let res = get_non_zero_tokens_for_chain(
                network.clone(),
                target_address,
                token_datas,
            ).await;
            
            let mut balance_list = match res {
                Ok(b_l) => b_l,
                Err(e) => {
                    tracing::error!("Error getting balance list: {:?}", e);
                    return;
                }
            };
            if !balance_list.is_empty() {
                get_token_prices(&network.chain_name, &mut balance_list).await.unwrap();
                let mut results = results_clone.lock().await;
                results.insert(network.chain_name.clone(), balance_list);
            }
        });

        while let Some(result) = handles.join_next().await {
            match result {
                Ok(_) => {}
                Err(e) => tracing::error!("Failed to fetch balance: {}", e),
            }
        }
    }

    let final_result = results.lock().await;
    // Output report
    output_report(&final_result);
    write_to_json_file(format!("results/tokens_{}.json", target_address), "results", &*final_result)?;
    Ok(())
}

async fn get_non_zero_tokens_for_chain(
    network: Network,
    target_wallet: Address,
    token_datas: Vec<TokenData>,
) -> Result<Vec<Balance>> {
    let provider = Arc::new(
        ProviderBuilder::new()
            .with_recommended_fillers()
            .on_http(network.rpc_url[0].clone()),
    );

    let mut web3_client = Web3Client::new(provider, None, network)?;
    
    let balance_list =  match web3_client.call_balance(target_wallet, token_datas).await {
        Ok(b_l) => b_l,
        Err(e) => {
            return Err(eyre::eyre!("Error getting balance list: {:?}", e));
        }
    };
    Ok(balance_list)
}

pub fn read_all_non_zero_balances() -> Result<()> {
    let dir = fs::read_dir("results")?;
    for entry in dir {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let file_name = path.file_name().unwrap().to_str().unwrap();
            if file_name.starts_with("tokens_") {
                let address = file_name.replace("tokens_", "").replace(".json", "");
                tracing::info!("Reading non zero balances for address: {}", address);
                read_non_zero_balances(address)?;
            }
        }
    }
    Ok(())
}

pub fn read_non_zero_balances(target_address: String) -> Result<()> {
    let balances = get_user_tokens_from_file(target_address)?;
    output_report(&balances);
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

async fn fetch_token_data(chain_name: &String) -> Result<Value> {
    let url = format!(
        "https://tokens.coingecko.com/{}/all.json",
        convert_network_name_to_coingecko_query_string(ChainName::from(chain_name.as_str()))
    );
    let url = Url::parse(&url)?;
    let res = reqwest::get(url).await?;
    let json: Value = res.json().await?;
    let token_data = match json.get("tokens") {
        Some(t_d) => t_d,
        None => return Err(eyre::eyre!("Token data is null")),
    };
    write_to_json_file(format!("data/token_lists/{}.json", chain_name), "data/token_lists", token_data)?;
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

async fn get_token_data(chain_name: &String) -> Result<Vec<TokenData>> {
    let token_list = match parse_json_data(format!("data/token_lists/{}.json", chain_name).as_str()) {
        Ok(v) => v,
        Err(_) => {
            tracing::warn!("No token data found for chain: {}. Fetching token data from Coingecko API", chain_name);
            match fetch_token_data(chain_name).await {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!("Error fetching token data: {:?}", e);
                    return Err(eyre::eyre!("Error fetching token data"));
                }
            }
        }
    };

    Ok(
        token_list.as_array().unwrap().iter().map(|token| {
            TokenData {
                address: token["address"].as_str().unwrap().parse::<Address>().unwrap_or(Address::ZERO),
                name: token["name"].as_str().unwrap().to_owned(),
                symbol: token["symbol"].as_str().unwrap().to_owned(),
                decimals: token["decimals"].as_u64().unwrap() as u8,
            }
        }).collect()
    )
}

    // async fn swap_tokens_to_native_for_chain(
    //     network: Network,
    //     token_in: TokenData,
    //     amount_in: U256,
    //     signer: PrivateKeySigner,
    //     balances: Vec<Balance>,
    // ) -> Result<()> {
    //     let native_token = vec![TokenData {
    //         address: "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE".parse().unwrap(),
    //         name: "Ether".to_owned(),
    //         symbol: "ETH".to_owned(),
    //         decimals: 18,
    //     }];

    //     let odos_aggregator = OdosAggregator::new(signer, network, vec![]).unwrap();

    //     // Arrays of size 5 to store tokens and amounts
    //     let mut tokens_in: Vec<TokenData> = Vec::with_capacity(5);
    //     let mut amounts_in: Vec<U256> = Vec::with_capacity(5);

    //     for (i, balance) in balances.iter().enumerate() {
    //         tokens_in.push(TokenData {
    //             address: balance.token_address,
    //             name: balance.token_name.clone(),
    //             symbol: balance.token_symbol.clone(),
    //             decimals: balance.decimals,
    //         });
    //         amounts_in.push(balance.balance);

    //         if tokens_in.len() == 5 || i == balances.len() - 1 {
    //             odos_aggregator.swap(tokens_in, native_token.clone(), amounts_in).await?;
    //             tokens_in.clear();
    //             amounts_in.clear();
    //         }
    //     }
    //     Ok(())
    // }


#[test]
fn test_read_non_zero_balances() {
    let _ = read_non_zero_balances("0xBF17a4730Fe4a1ea36Cf536B8473Cc25ba146F19".to_owned()).unwrap();
}

// #[tokio::test]
// async fn test_get_non_zero_tokens() {
//     dotenv::dotenv().ok();
//     let _ = get_non_zero_tokens("0xf63feA8d383b8089BAbFf2A712AB3190CB21732D".parse().unwrap()).await.unwrap();
// }

#[tokio::test]
async fn test_token_fetch() -> Result<()> {
    let tn = "Manta".to_owned();
    let d = get_token_data(&tn).await?;
    println!("{:?}", d.len());
    Ok(())
}

#[tokio::test]
async fn test_get_native_token_price() -> Result<()> {
    let balance = Balance::new(
        "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE".parse::<Address>().unwrap(),
        "NATIVE (ETH)".to_owned(),
        "NATIVE (ETH)".to_owned(),
        18,
        alloy::primitives::U256::from(1000000000),
    );
    let mut balances = vec![balance];
    let _ = get_token_prices("Ethereum", &mut balances).await?;
    println!("{:?}", balances[0].token_price);
    Ok(())
}