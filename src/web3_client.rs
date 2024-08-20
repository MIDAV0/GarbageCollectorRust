use std::{fs,time};

use alloy::{
    contract::Interface, dyn_abi::DynSolValue, json_abi::JsonAbi, network::{Ethereum, EthereumWallet, TransactionBuilder}, primitives::{Address, Bytes, U256}, providers::{
        fillers::{ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller}, Identity, Provider, ProviderBuilder, RootProvider
    }, rpc::types::{TransactionReceipt, TransactionRequest}, signers::local::PrivateKeySigner, sol, transports::http::{Client, Http}
};
use eyre::Result;
use reqwest::Url;
use serde::{Serialize, Deserialize};

use crate::TokenData;

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

#[derive(Clone)]
pub struct Network {
    pub id: u32,
    pub chain_name: String,
    pub rpc_url: Vec<Url>,
    pub explorer: String,
    pub multicall: Address,
}

impl Network {
    pub fn new(
        id: u32,
        chain_name: String,
        rpc_url: Vec<Url>,
        explorer: String,
        multicall: Address,
    ) -> Result<Self> {
        if rpc_url.is_empty() {
            return Err(eyre::eyre!("RPC URL is required"));
        }

        Ok(Network {
            id,
            chain_name,
            rpc_url,
            explorer,
            multicall,
        })
    }
}

pub struct GasMultiplier {
    price: f32,
    limit: f32,
}

impl GasMultiplier {
    pub fn new(price: f32, limit: f32) -> Self {
        GasMultiplier {
            price,
            limit,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Balance {
    pub token_address: Address,
    pub balance: U256,
    pub token_name: String,
    pub token_symbol: String,
    pub token_price: f64,
    pub decimals: u8,
}

impl Balance {
    pub fn new(
        token_address: Address,
        token_name: String,
        token_symbol: String,
        decimals: u8,
        balance: U256
    ) -> Self {
        Balance {
            token_address,
            token_name,
            token_symbol,
            decimals,
            balance,
            token_price: 0.0,
        }
    }

    pub fn set_token_price(&mut self, price: f64) {
        self.token_price = price;
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

    pub async fn approve(
        &self,
        token_address: Address,
        to: Address,
        amount: U256,
        _min_allowance: Option<U256>,
    ) -> Result<Option<TransactionReceipt>> {
        let provider = self.get_provider(0);
        let erc20 = ERC20::new(token_address, provider);

        if let Some(min_allowance) = _min_allowance {
            let ERC20::allowanceReturn { _0 } = erc20.allowance(self.signer.address(), to).call().await?;
            // Print allowance data

            if _0 >= min_allowance {
                return Ok(None);
            }
        }

        let call_data = Bytes::copy_from_slice(&self.erc20_interface.encode_input("approve", &[
            DynSolValue::Address(to),
            DynSolValue::Uint(amount, 256),
        ])?);
        let tx = TransactionRequest::default()
            .with_from(self.signer.address())
            .with_to(token_address)
            .with_input(call_data);

        let tx_hash = self.send_tx(tx, None).await?;
        
        Ok(Some(tx_hash))
    }

    pub async fn send_tx(
        &self,
        mut tx_body: TransactionRequest,
        _gas_multipliers: Option<GasMultiplier>,
    ) -> Result<TransactionReceipt> {
        let provider = self.get_provider(0);
        if let Some(gas_multipliers) = _gas_multipliers {
            let gas_limit = self.estimate_tx_gas(&tx_body, Some(gas_multipliers.limit)).await?;
            let gas_price = self.get_gas_price(Some(gas_multipliers.price)).await?;
            tx_body = tx_body.gas_limit(gas_limit).max_fee_per_gas(gas_price);

            let wallet = EthereumWallet::from(self.signer.clone());
            let tx_envelope = tx_body.build(&wallet).await?;
            let tx_receipt = provider.send_tx_envelope(tx_envelope).await?.get_receipt().await?;
            Ok(tx_receipt)  
        } else {
            let tx_receipt = provider.send_transaction(tx_body).await?.get_receipt().await?;
            Ok(tx_receipt)          
        }
    }

    async fn estimate_tx_gas(
        &self,
        tx_body: &TransactionRequest,
        _multiplier: Option<f32>,
    ) -> Result<u128> {
        let multiplier = _multiplier.unwrap_or(1.3);
        let provider = self.get_provider(0);
        let gas_estimate = provider.estimate_gas(tx_body).await?;
        Ok((gas_estimate as f32 * multiplier) as u128)
    }

    async fn get_gas_price(
        &self,
        _multiplier: Option<f32>,
    ) -> Result<u128> {
        let multiplier = _multiplier.unwrap_or(1.3);
        let provider = self.get_provider(0);
        let gas_price = provider.get_gas_price().await?;
        Ok((gas_price as f32 * multiplier) as u128)
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

    pub async fn call_balance(&self, wallet_address: Address, tokens: Vec<TokenData>) -> Result<Vec<Balance>> {
        let provider = self.get_provider(0);
        let mut multicall = Multicall::new(self.network.multicall, provider);
        let max_retries = 2;
        let mut balances: Vec<Balance> = vec![];
        let mut calls: Vec<Multicall::Call> = vec![];
        let mut token_buffer: Vec<&TokenData> = vec![];
        let batch_size = 500;
        for (index, token) in tokens.iter().enumerate() {
            if token.address == "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE".parse::<Address>().unwrap() {
                let call_data = Bytes::copy_from_slice(&self.multicall_interface.encode_input("getEthBalance", &[
                    DynSolValue::Address(wallet_address)
                ])?);
                calls.push(Multicall::Call {
                    target: self.network.multicall,
                    callData: call_data,
                });
                token_buffer.push(token);
            } else {
                let call_data = Bytes::copy_from_slice(&self.erc20_interface.encode_input("balanceOf", &[
                    DynSolValue::Address(wallet_address)
                ])?);
                calls.push(Multicall::Call {
                    target: token.address,
                    callData: call_data,
                });
                token_buffer.push(token);
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
                        if !balance_data.success || &balance_data.returnData[..] == b"0x" {
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
 
                        if !balance.is_zero() {
                            balances.push(
                                Balance::new(
                                    token_buffer[i].address,
                                    token_buffer[i].name.clone(),
                                    token_buffer[i].symbol.clone(),
                                    token_buffer[i].decimals,
                                    balance
                                )
                            );
                        }
                    }
                    break;
                }
                calls.clear();
                token_buffer.clear();
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
async fn test_get_balance() {
    let signer = PrivateKeySigner::random();
    let web3_client = Web3Client::new(
        Network {
            id: 1,
            chain_name: "Ethereum".to_owned(),
            rpc_url: vec!["https://ethereum.publicnode.com".parse::<Url>().unwrap()],
            explorer: "https://etherscan.io/tx/".to_owned(),
            multicall: "0xcA11bde05977b3631167028862bE2a173976CA11".parse().unwrap(),
        },
        signer,
    ).unwrap();
    let balance = web3_client.get_user_balance("0xBF17a4730Fe4a1ea36Cf536B8473Cc25ba146F19".parse().unwrap(), None).await.unwrap();
    println!("{:?}", balance);
}

#[tokio::test]
async fn test_approve() {
    let signer: PrivateKeySigner = "".parse().expect("should parse private key");
    println!("{:?}", signer.address());
    let web3_client = Web3Client::new(
        Network {
            id: 1,
            chain_name: "Ethereum".to_owned(),
            rpc_url: vec!["https://ethereum.publicnode.com".parse::<Url>().unwrap()],
            explorer: "https://etherscan.io/tx/".to_owned(),
            multicall: "0xcA11bde05977b3631167028862bE2a173976CA11".parse().unwrap(),
        },
        signer,
    ).unwrap();
    let result = web3_client.approve(
        "0x6ff2241756549b5816a177659e766eaf14b34429".parse().unwrap(),
        "0xBF17a4730Fe4a1ea36Cf536B8473Cc25ba146F19".parse().unwrap(),
        U256::from(1000000000000000_i64),
        None,
    ).await.unwrap();
    println!("{:?}", result);
}