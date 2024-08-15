

pub struct OdosAggregator {
    signer: PrivateKeySigner,
    network: Network,
    proxies: Vec<Proxy>,
    quote_url: String,
    assemble_url: String,
}

impl OdosAggregator {
    pub fn new(
        signer: PrivateKeySigner,
        network: Network,
        proxies: Vec<Proxy>,
    ) -> Self {
        OdosAggregator {
            signer,
            network,
            proxies,
            quote_url: "https://api.odos.xyz/sor/quote/v2".to_owned(),
            assemble_url: "https://api.odos.xyz/sor/assemble".to_owned(),
        }
    }
}