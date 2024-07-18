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

struct Web3Client {
    provider: MyFiller,
    network_name: ChainName,
    with_signer: bool,
}

impl Web3Client {
    pub fn new(
        rpc_url_: &str,
        chain_name: &str,
        private_key: Option<&str>,
    ) -> Result<Self> {
        let network_name = ChainName::from(chain_name);
        let rpc_url = rpc_url_.parse()?;
        let signer: PrivateKeySigner = if let Some(pk) = private_key {
            pk.parse().expect("invalid private key")
        } else {
            PrivateKeySigner::random()
        };
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_http(rpc_url);
        Ok(
            Web3Client {
                provider,
                network_name,
                with_signer: private_key.is_some(),
            }
        )
    }

    pub async fn get_erc20_token_balance(&self, token_address: &str, address: &str) -> Result<U256> {
        let contract = ERC20::new(token_address.parse()?, self.provider.clone());
        let ERC20::balanceOfReturn { balance } = contract.balanceOf(address.parse::<Address>()?).call().await?;

        Ok(balance)
    }
}