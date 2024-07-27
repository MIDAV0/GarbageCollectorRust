use std::{io::Read, ops::Add};

use alloy::{
    contract::Interface, dyn_abi::DynSolValue, json_abi::JsonAbi, network::{Ethereum, EthereumWallet}, primitives::{ Address, Bytes, U256 }, providers::{
        fillers::{ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller}, Identity, Provider, ProviderBuilder, RootProvider
    }, signers::local::PrivateKeySigner, sol, transports::http::{Client, Http}
};
use eyre::Result;
use crate::const_types::ChainName;

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

#[derive(Default)]
pub struct Network {
    id: i32,
    lz_id: String,
    rpc_url: String,
    explorer: String,
    multicall: String,
}

impl Network {
    pub fn new(
        id: i32,
        lz_id: String,
        rpc_url: String,
        explorer: String,
        multicall: String,
    ) -> Self {
        Network {
            id,
            lz_id,
            rpc_url,
            explorer,
            multicall,
        }
    }
}

#[derive(Debug)]
pub struct Balance {
    token_address: String,
    balance: U256,
}

pub struct Web3Client {
    network_name: ChainName,
    network: Network,
    signer: PrivateKeySigner,
    multicall_interface: Interface,
    erc20_interface: Interface,
}

impl Web3Client {
    pub fn new(
        chain_name: &str,
        signer_: Option<PrivateKeySigner>,
    ) -> Result<Self> {
        let multicall_interface = {
            let path = "src/utils/contract_abis/Multicall2.json";
            let json = std::fs::read_to_string(path)?;
            let abi: JsonAbi = serde_json::from_str(&json)?;
            Interface::new(abi)
        };
        
        let erc20_interface = {
            let path = "src/utils/contract_abis/ERC20.json";
            let json = std::fs::read_to_string(path)?;
            let abi: JsonAbi = serde_json::from_str(&json)?;
            Interface::new(abi)
        };

        let network_name = ChainName::from(chain_name);
        let signer = signer_.unwrap_or(PrivateKeySigner::random());
        Ok(
            Web3Client {
                network_name,
                network: Network::default(),
                signer,
                multicall_interface,
                erc20_interface,
            }
        )
    }

    pub fn set_network_rpc(&mut self, network_: Network) {
        self.network = network_;
    }

    fn get_provider(&self) -> Result<MyFiller> {
        let rpc_url = self.network.rpc_url.clone().parse()?;
        let wallet = EthereumWallet::from(self.signer.clone());
        Ok(ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_http(rpc_url))
    }

    pub async fn get_user_balance(provider: MyFiller, wallet_address: Address, token_address: Option<String>) -> Result<U256> {
        if let Some(token) = token_address {
            let erc20 = ERC20::new(token.parse()?, provider.clone());
            let ERC20::balanceOfReturn { balance } = erc20.balanceOf(wallet_address).call().await?;
            Ok(balance)
        } else {
            Ok(provider.get_balance(wallet_address).await?)
        }
    }

    pub async fn call_balance(&self, wallet_address: Address, tokens: Vec<String>) -> Result<Vec<Balance>> {
        let provider: MyFiller = self.get_provider()?;
        let multicall = Multicall::new(self.network.multicall.parse()?, provider.clone());
        let max_retries = 2;
        let mut balances: Vec<Balance> = vec![];
        let mut calls: Vec<Multicall::Call> = vec![];
        let batch_size = 500;
        for (index, token) in tokens.iter().enumerate() {
            let token_address = token.parse::<Address>()?;
            if token.to_lowercase() == "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE".to_owned().to_lowercase() {
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
                            // Sleep for 5 seconds
                            continue;
                        }
                    };
                    for (index, balance_data) in returnData.iter().enumerate() {
                        if balance_data.success == false {
                            println!("Failed to get balance for token: {}", tokens[index]);
                            continue;
                        }
                        let balance = match U256::try_from_be_slice(&balance_data.returnData) {
                            Some(b) => b,
                            None => {
                                println!("Failed to convert balance data to U256 for token: {}", tokens[index]);
                                continue;
                            }
                        };
                        if balance > U256::from(0) {
                            balances.push(Balance {
                                token_address: tokens[index].clone(),
                                balance,
                            });
                        }
                    }
                    break;
                }
                calls.clear();
                // Sleep for 0.2 seconds
            }
        }

        Ok(balances)
    }
}

#[test]
fn test_encode_function_data() {
    let multicall_interface = {
        let path = "src/utils/contract_abis/Multicall2.json";
        let json = std::fs::read_to_string(path).unwrap();
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
    let mut web3_client = Web3Client::new("Ethereum", None).unwrap();
    web3_client.set_network_rpc(Network {
        id: 1,
        lz_id: "101".to_owned(),
        rpc_url: "https://ethereum.publicnode.com".to_owned(),
        explorer: "https://etherscan.io/tx/".to_owned(),
        multicall: "0xcA11bde05977b3631167028862bE2a173976CA11".to_owned(),
    });
    let _ = web3_client.call_balance("0xBF17a4730Fe4a1ea36Cf536B8473Cc25ba146F19".parse().unwrap(), vec!["0x6FF2241756549B5816A177659E766EAf14B34429".to_owned()]).await;
}

#[tokio::test]
async fn test_get_balance() {
    let mut web3_client = Web3Client::new("Ethereum", None).unwrap();
    web3_client.set_network_rpc(Network {
        id: 1,
        lz_id: "101".to_owned(),
        rpc_url: "https://ethereum.publicnode.com".to_owned(),
        explorer: "https://etherscan.io/tx/".to_owned(),
        multicall: "0xcA11bde05977b3631167028862bE2a173976CA11".to_owned(),
    });
    let provider: MyFiller = web3_client.get_provider().unwrap();
    let balance = Web3Client::get_user_balance(provider, "0xBF17a4730Fe4a1ea36Cf536B8473Cc25ba146F19".parse().unwrap(), None).await.unwrap();
    println!("{:?}", balance);
}