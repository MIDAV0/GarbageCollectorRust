use std::{str::FromStr, sync::Arc};
use alloy::{primitives::Address, signers::local::PrivateKeySigner};
use reqwest::Proxy;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct Account {
    private_key: String,
    proxy: Option<String>,
    address: String,
}

impl Account {
    pub fn new(
        private_key: &str,
        proxy: Option<String>,
    ) -> Self {
        let signer = Arc::new(PrivateKeySigner::from_str(private_key).expect(format!("Private key {} to be valid", private_key).as_str()));
        let address = signer.address();

        Self {
            private_key: private_key.to_string(),
            proxy,
            address: address.to_string(),
            ..Default::default()
        }
    }

    pub fn proxy(&self) -> Option<Proxy> {
        self.proxy
            .as_ref()
            .map(|proxy| Proxy::all(proxy).expect("Proxy to be valid"))
    }

    pub fn signer(&self) -> Arc<PrivateKeySigner> {
        Arc::new(PrivateKeySigner::from_str(&self.private_key).unwrap())
    }

    pub fn get_address(&self) -> Address {
        Address::from_str(&self.address).expect("Address to be valid")
    }
}