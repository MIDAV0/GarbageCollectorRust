use std::{fs,time};

use alloy::{
    contract::Interface, dyn_abi::DynSolValue, json_abi::JsonAbi, network::{Ethereum, EthereumWallet}, primitives::{ Address, Bytes, U256 }, providers::{
        fillers::{ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller}, Identity, Provider, ProviderBuilder, RootProvider
    }, signers::local::PrivateKeySigner, sol, transports::http::{Client, Http}
};
use eyre::Result;
use reqwest::Url;
use serde::{Serialize, Deserialize};

type MyFiller = FillProvider<JoinFill<JoinFill<JoinFill<JoinFill<Identity, GasFiller>, NonceFiller>, ChainIdFiller>, WalletFiller<EthereumWallet>>, RootProvider<Http<Client>>, Http<Client>, Ethereum>;

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    ERC20,
    "src/utils/contract_abis/ERC20.json"
);

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    Multicall,
    "src/utils/contract_abis/Multicall2.json"
);

pub struct Network {
    id: i32,
    lz_id: String,
    rpc_url: Vec<Url>,
    explorer: String,
    multicall: Address,
}

impl Network {
    pub fn new(
        id: i32,
        lz_id: String,
        rpc_url: Vec<Url>,
        explorer: String,
        multicall: Address,
    ) -> Result<Self> {
        if rpc_url.is_empty() {
            return Err(eyre::eyre!("RPC URL is required"));
        }

        Ok(Network {
            id,
            lz_id,
            rpc_url,
            explorer,
            multicall,
        })
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Balance {
    pub token_address: Address,
    pub balance: U256,
    pub token_symbol: String,
    pub token_price: f64,
    pub decimals: u8,
}

impl Balance {
    pub fn new(token_address: Address, balance: U256) -> Self {
        Balance {
            token_address,
            balance,
            ..Default::default()
        }
    }

    pub fn set_token_price(&mut self, price: f64) {
        self.token_price = price;
    }

    pub fn set_token_symbol(&mut self, symbol: String) {
        self.token_symbol = symbol;
    }

    pub fn set_decimals(&mut self, decimals: u8) {
        self.decimals = decimals;
    }
}

pub struct Web3Client {
    signer: PrivateKeySigner,
    network: Network,
    multicall_interface: Interface,
    erc20_interface: Interface,
}

impl Web3Client {
    pub fn new(
        network: Network,
        signer: PrivateKeySigner,
    ) -> Result<Self> {
        let multicall_interface = {
            let path = "src/utils/contract_abis/Multicall2.json";
            let json = fs::read_to_string(path)?;
            let abi: JsonAbi = serde_json::from_str(&json)?;
            Interface::new(abi)
        };
        
        let erc20_interface = {
            let path = "src/utils/contract_abis/ERC20.json";
            let json = fs::read_to_string(path)?;
            let abi: JsonAbi = serde_json::from_str(&json)?;
            Interface::new(abi)
        };

        Ok(
            Web3Client {
                signer,
                network,
                multicall_interface,
                erc20_interface,
            }
        )
    }

    // Sleep function
    async fn sleep(duration: time::Duration) {
        tokio::time::sleep(duration).await;
    }

    fn get_provider(&self, retry_count: usize) -> MyFiller {
        let wallet = EthereumWallet::from(self.signer.clone());
        // Get the rpc url index based on the retry count using modulo operator
        let index = retry_count % self.network.rpc_url.len();
        let rpc_url = self.network.rpc_url[index].clone();
        ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_http(rpc_url)
    }

    pub async fn get_user_balance(&self, wallet_address: Address, token_address: Option<String>) -> Result<U256> {
        let provider = self.get_provider(0);
        if let Some(token) = token_address {
            let erc20 = ERC20::new(token.parse()?, provider);
            let ERC20::balanceOfReturn { balance } = erc20.balanceOf(wallet_address).call().await?;
            Ok(balance)
        } else {
            Ok(provider.get_balance(wallet_address).await?)
        }
    }

    pub async fn call_balance(&self, wallet_address: Address, tokens: Vec<Address>) -> Result<Vec<Balance>> {
        let provider = self.get_provider(0);
        let mut multicall = Multicall::new(self.network.multicall, provider);
        let max_retries = 2;
        let mut balances: Vec<Balance> = vec![];
        let mut calls: Vec<Multicall::Call> = vec![];
        let batch_size = 500;
        for (index, token) in tokens.iter().enumerate() {
            let token_address = *token;
            if token == &Address::ZERO {
                let call_data = Bytes::copy_from_slice(&self.multicall_interface.encode_input("getEthBalance", &[
                    DynSolValue::Address(wallet_address)
                ])?);
                calls.push(Multicall::Call {
                    target: token_address,
                    callData: call_data,
                });
            } else {
                let call_data = Bytes::copy_from_slice(&self.erc20_interface.encode_input("balanceOf", &[
                    DynSolValue::Address(wallet_address)
                ])?);
                calls.push(Multicall::Call {
                    target: token_address,
                    callData: call_data,
                });
            }

            // If batch size is reached or if it's the last token in the list then aggregate the calls
            if ((index + 1) % batch_size == 0 && index != 0) || index + 1 >= tokens.len() {
                // Aggregate the calls
                let mut retry_count = 0;
                while retry_count < max_retries {
                    let call_result = multicall.tryAggregate(false, calls.clone()).call().await;
                    let Multicall::tryAggregateReturn { returnData } = match call_result {
                        Ok(data) => data,
                        Err(e) => {
                            println!("Error: {:?}", e);
                            retry_count += 1;
                            if self.network.rpc_url.len() == 1 {
                                Self::sleep(time::Duration::from_millis(3000)).await;
                            } else {
                                let new_provider = self.get_provider(retry_count);
                                multicall = Multicall::new(self.network.multicall, new_provider);
                            }
                            continue;
                        }
                    };
                    for (i, balance_data) in returnData.iter().enumerate() {
                        if !balance_data.success {
                            continue;
                        }

                        // Multicall could return more bytes then needed for U256
                        let value = if balance_data.returnData.len() > 66 {
                            &balance_data.returnData[0..66]
                        } else {
                            &balance_data.returnData
                        };
                        let balance = match U256::try_from_be_slice(value) {
                            Some(b) => b,
                            None => continue,
                        };
                        if balance > U256::from(0) {
                            let current_index = if index + 1 >= batch_size {
                                (index + 1) - batch_size + i
                            } else {
                                i
                            };
                            balances.push(Balance::new(tokens[current_index], balance));
                        }
                    }
                    break;
                }
                calls.clear();
                Self::sleep(time::Duration::from_millis(200)).await;
            }
        }
        Ok(balances)
    }
}

#[test]
fn test_encode_function_data() {
    let multicall_interface = {
        let path = "src/utils/contract_abis/Multicall2.json";
        let json = fs::read_to_string(path).unwrap();
        let abi: JsonAbi = serde_json::from_str(&json).unwrap();
        Interface::new(abi)
    };
    let address = "0xBF17a4730Fe4a1ea36Cf536B8473Cc25ba146F19";

    let result = multicall_interface.encode_input("getEthBalance", &[
        DynSolValue::Address(address.parse().unwrap())
    ]).unwrap();
    println!("{:?}", result);
}

#[tokio::test]
async fn test_call_balance() {
    let signer = PrivateKeySigner::random();
    let web3_client = Web3Client::new(
        Network {
            id: 1,
            lz_id: "101".to_owned(),
            rpc_url: vec!["https://ethereum.publicnode.com".parse::<Url>().unwrap()],
            explorer: "https://etherscan.io/tx/".to_owned(),
            multicall: "0xcA11bde05977b3631167028862bE2a173976CA11".parse().unwrap(),
        },
        signer,
    ).unwrap();

    let _ = web3_client.call_balance("0xBF17a4730Fe4a1ea36Cf536B8473Cc25ba146F19".parse().unwrap(), vec!["0x6FF2241756549B5816A177659E766EAf14B34429".parse::<Address>().unwrap()]).await;
}

#[tokio::test]
async fn test_get_balance() {
    let signer = PrivateKeySigner::random();
    let web3_client = Web3Client::new(
        Network {
            id: 1,
            lz_id: "101".to_owned(),
            rpc_url: vec!["https://ethereum.publicnode.com".parse::<Url>().unwrap()],
            explorer: "https://etherscan.io/tx/".to_owned(),
            multicall: "0xcA11bde05977b3631167028862bE2a173976CA11".parse().unwrap(),
        },
        signer,
    ).unwrap();
    let balance = web3_client.get_user_balance("0xBF17a4730Fe4a1ea36Cf536B8473Cc25ba146F19".parse().unwrap(), None).await.unwrap();
    println!("{:?}", balance);
}