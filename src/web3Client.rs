use alloy::{
    network::{Ethereum, EthereumWallet},
    primitives::{Address, U128, U256, U64},
    providers::{fillers::{ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller}, Identity, ProviderBuilder, RootProvider},
    signers::local::PrivateKeySigner,
    sol, transports::http::{Client, Http},
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

pub struct Web3Client {
    network_name: ChainName,
    network_rpc_url: String,
    signer: PrivateKeySigner,
}

impl Web3Client {
    pub fn new(
        chain_name: &str,
        signer_: Option<PrivateKeySigner>,
    ) -> Result<Self> {
        let network_name = ChainName::from(chain_name);
        let signer = signer_.unwrap_or(PrivateKeySigner::random());
        Ok(
            Web3Client {
                network_name,
                network_rpc_url: "".to_string(),
                signer,
            }
        )
    }

    pub fn set_network_rpc(&mut self, network_rpc_url_: &str) {
        self.network_rpc_url = network_rpc_url_.to_string();
    }

    fn get_provider(&self) -> Result<MyFiller> {
        let rpc_url = self.network_rpc_url.clone().parse()?;
        let wallet = EthereumWallet::from(self.signer.clone());
        Ok(ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_http(rpc_url))
    }

    // pub async fn get_erc20_token_balance(&self, token_address: &str, address: &str) -> Result<U256> {
    //     let contract = ERC20::new(token_address.parse()?, self.provider.clone());
    //     let ERC20::balanceOfReturn { balance } = contract.balanceOf(address.parse::<Address>()?).call().await?;

    //     Ok(balance)
    // }
}