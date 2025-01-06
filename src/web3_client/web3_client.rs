use std::{marker::PhantomData, fs, sync::Arc, time};

use alloy::{
    contract::Interface, dyn_abi::DynSolValue, json_abi::JsonAbi, network::{Ethereum, EthereumWallet, TransactionBuilder}, primitives::{Address, Bytes, U256}, providers::{utils::Eip1559Estimation, Provider}, rpc::types::{TransactionReceipt, TransactionRequest}, signers::local::PrivateKeySigner, sol, sol_types::sol_data::Bool, transports::{Transport, TransportErrorKind}
};
use alloy_json_rpc::RpcError;
use eyre::Result;
use log::warn;
use serde::{Serialize, Deserialize};

use crate::constants::{TokenData, Network};
use crate::helpers::utils::{change_rpc, retry_async, sleep};

use super::constants::{MULTICALL_ABI_PATH, ERC20_ABI_PATH};

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

pub struct Web3Client<P, T>
where 
    P: Provider<T, Ethereum>,
    T: Transport + Clone,
{
    provider: Arc<P>,
    network: Network,
    signer: PrivateKeySigner,
    wallet: EthereumWallet,
    multicall_interface: Interface,
    erc20_interface: Interface,
    _marker: PhantomData<T>,
}

impl<P, T> Web3Client<P, T>
where 
    P: Provider<T, Ethereum>,
    T: Transport + Clone,
{
    pub fn new(
        provider: Arc<P>,
        private_key: Option<&str>,
        network: Network,
    ) -> Result<Self> {
        let multicall_interface = {
            let json = fs::read_to_string(MULTICALL_ABI_PATH)?;
            let abi: JsonAbi = serde_json::from_str(&json)?;
            Interface::new(abi)
        };
        
        let erc20_interface = {
            let json = fs::read_to_string(ERC20_ABI_PATH)?;
            let abi: JsonAbi = serde_json::from_str(&json)?;
            Interface::new(abi)
        };

        let signer = match private_key {
            Some(key) => key.parse().expect("Private key to be valid"),
            None => PrivateKeySigner::random(),
        };

        let wallet = EthereumWallet::from(signer.clone());

        Ok(
            Web3Client {
                provider,
                network,
                signer,
                wallet,
                multicall_interface,
                erc20_interface,
                _marker: PhantomData,
            }
        )
    }

    pub fn address(&self) -> Address {
        self.signer.address()
    }

    pub async fn approve(
        &mut self,
        token_address: Address,
        to: Address,
        amount: U256,
        _min_allowance: Option<U256>,
    ) -> Result<bool> {
        if let Some(min_allowance) = _min_allowance {
            let result = retry_async(
                |_| { ERC20::new(token_address, self.provider).allowance(signer_address, to).call() },
                3,
                1000,
            ).await?;

            let ERC20::allowanceReturn { _0 } = result;
            if _0 >= min_allowance {
                return true;
            }
        }

        let call_data = Bytes::copy_from_slice(&self.erc20_interface.encode_input("transfer", &[
            DynSolValue::Address(to),
            DynSolValue::Uint(amount, 256),
        ])?);

        let tx_result = self.send_tx(token_address, Some(call_data), None).await?;
        
        Ok(tx_result)
    }

    pub async fn send_tx(
        &mut self,
        to: Address,
        input: Option<Bytes>,
        gas_multipliers: Option<GasMultiplier>,
    ) -> Result<bool> {
        let mut eip1559_fees = self.provider.estimate_eip1559_fees(None).await?;

        if let Some(multipliers) = gas_multipliers {
            eip1559_fees.max_fee_per_gas = (eip1559_fees.max_fee_per_gas as f32 * multipliers.price) as u128;
            eip1559_fees.max_priority_fee_per_gas = (eip1559_fees.max_priority_fee_per_gas as f32 * multipliers.price) as u128;
        }

        let nonce = self
            .provider
            .get_transaction_count(self.signer.address())
            .await?;

        let mut tx_request = TransactionRequest::default()
            .with_max_fee_per_gas(eip1559_fees.max_fee_per_gas)
            .with_max_priority_fee_per_gas(eip1559_fees.max_priority_fee_per_gas)
            .with_to(to)
            .with_nonce(nonce)
            .with_chain_id(self.network.id as u64)
            .with_from(self.address());

        if let Some(data) = input {
            tx_request.set_input(data);
        }

        let gas_limit = self.provider.estimate_gas(&tx_request).await?;
        tx_request.set_gas_limit(gas_limit);

        let signed_transaction = tx_request.build(&self.wallet).await?;

        let receipt = retry_async(
            |_| { self.provider.send_tx_envelope(signed_transaction.clone()) },
            3,
            1000,
        ).await?.get_receipt().await?;


        let tx_status = receipt.status();
        if tx_status {
            tracing::info!(
                "Transaction successful: {}/tx/{}",
                self.network.explorer,
                receipt.transaction_hash
            );
        } else {
            tracing::error!("Transaction failed: {}/tx/{}",
                self.network.explorer,
                receipt.transaction_hash
            );
        }

        Ok(tx_status)
    }

    async fn estimate_tx_gas(
        &mut self,
        tx_body: &TransactionRequest,
        _multiplier: Option<f32>,
    ) -> Result<u128, RpcError<TransportErrorKind>> {
        let multiplier = _multiplier.unwrap_or(1.3);
        let wallet = EthereumWallet::from(self.signer.clone());
        let gas_estimate = retry_async(
            |x| {
                let provider = change_rpc(wallet.clone(), &self.network.rpc_url, x);
                async move { 
                    provider.estimate_gas(tx_body).await
                }                  
            },
            3,
            1000,
        ).await?;
        Ok((gas_estimate as f32 * multiplier) as u128)
    }

    async fn get_gas_price(
        &self,
        _multiplier: Option<f32>,
    ) -> Result<u128> {
        let multiplier = _multiplier.unwrap_or(1.3);
        let gas_price = self.provider.get_gas_price().await?;
        Ok((gas_price as f32 * multiplier) as u128)
    }

    pub async fn get_user_balance(&self, wallet_address: Address, token_address: Option<String>) -> Result<U256> {
        if let Some(token) = token_address {
            let erc20 = ERC20::new(token.parse()?, self.provider.clone());
            let ERC20::balanceOfReturn { balance } = erc20.balanceOf(wallet_address).call().await?;
            Ok(balance)
        } else {
            Ok(self.provider.get_balance(wallet_address).await?)
        }
    }

    pub async fn call_balance(&mut self, wallet_address: Address, tokens: Vec<TokenData>) -> Result<Vec<Balance>> {
        let mut multicall = Multicall::new(self.network.multicall, self.provider.clone());
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
                        Err(_) => {
                            retry_count += 1;
                            warn!("RPC call failed. Trying again. Retry count: {}", retry_count+1);
                            if self.network.rpc_url.len() == 1 {
                                sleep(time::Duration::from_millis(3000)).await;
                            } else {
                                // self.change_rpc(retry_count);
                                multicall = Multicall::new(self.network.multicall, self.provider.clone());
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
                sleep(time::Duration::from_millis(200)).await;
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
        Some(signer),
    ).unwrap();
    let balance = web3_client.get_user_balance("0xBF17a4730Fe4a1ea36Cf536B8473Cc25ba146F19".parse().unwrap(), None).await.unwrap();
    println!("{:?}", balance);
}

#[tokio::test]
async fn test_approve() {
    let signer: PrivateKeySigner = "".parse().expect("should parse private key");
    println!("{:?}", signer.address());
    let mut web3_client = Web3Client::new(
        Network {
            id: 1,
            chain_name: "Ethereum".to_owned(),
            rpc_url: vec!["https://ethereum.publicnode.com".parse::<Url>().unwrap()],
            explorer: "https://etherscan.io/tx/".to_owned(),
            multicall: "0xcA11bde05977b3631167028862bE2a173976CA11".parse().unwrap(),
        },
        Some(signer),
    ).unwrap();
    let result = web3_client.approve(
        "0x6ff2241756549b5816a177659e766eaf14b34429".parse().unwrap(),
        "0xBF17a4730Fe4a1ea36Cf536B8473Cc25ba146F19".parse().unwrap(),
        U256::from(1000000000000000_i64),
        None,
    ).await.unwrap();
    println!("{:?}", result);
}