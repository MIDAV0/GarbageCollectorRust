
pub static PROJECT_NAME: &str = "garbage_collector";

#[derive(Debug, Clone)]
pub struct Env {
    pub debug: bool,
}

pub fn get_env(key: &str) -> String {
    std::env::var(key).unwrap_or(String::from(""))
}

impl Env {
    pub fn new() -> Self {
        Env {
            debug: get_env("DEBUG").parse::<bool>().unwrap(),
        }
    }
}

pub enum ChainName {
    Ethereum,
    Arbitrum,
    Optimism,
    Base,
    Linea,
    Zksync,
    Bsc,
    Opbnb,
    Polygon,
    Avalanche,
    Scroll,
    Blast,
    Mantle,
    Gnosis,
    Fantom,
    Celo,
    Core,
    Manta,
    Taiko,
    // | 'Zora',
    Nova,
}

impl From<&str> for ChainName {
    fn from(s: &str) -> Self {
        match s {
            "Ethereum" => ChainName::Ethereum,
            "Arbitrum" => ChainName::Arbitrum,
            "Optimism" => ChainName::Optimism,
            "Base" => ChainName::Base,
            "Linea" => ChainName::Linea,
            "Zksync" => ChainName::Zksync,
            "Bsc" => ChainName::Bsc,
            "Opbnb" => ChainName::Opbnb,
            "Polygon" => ChainName::Polygon,
            "Avalanche" => ChainName::Avalanche,
            "Scroll" => ChainName::Scroll,
            "Blast" => ChainName::Blast,
            "Mantle" => ChainName::Mantle,
            "Gnosis" => ChainName::Gnosis,
            "Fantom" => ChainName::Fantom,
            "Celo" => ChainName::Celo,
            "Core" => ChainName::Core,
            "Manta" => ChainName::Manta,
            "Taiko" => ChainName::Taiko,
            "Nova" => ChainName::Nova,
            _ => panic!("Invalid chain name"),
        }
    }
}

pub fn convert_network_name_to_coingecko_query_string(chain_name: ChainName) -> String {
    match chain_name {
        ChainName::Ethereum => "ethereum".to_owned(),
        ChainName::Arbitrum => "arbitrum-one".to_owned(),
        ChainName::Optimism => "optimistic-ethereum".to_owned(),
        ChainName::Base => "base".to_owned(),
        ChainName::Linea => "linea".to_owned(),
        ChainName::Zksync => "zksync".to_owned(),
        ChainName::Bsc => "binance-smart-chain".to_owned(),
        ChainName::Opbnb => "opbnb".to_owned(),
        ChainName::Polygon => "polygon-pos".to_owned(),
        ChainName::Avalanche => "avalanche".to_owned(),
        ChainName::Scroll => "scroll".to_owned(),
        ChainName::Blast => "blast".to_owned(),
        ChainName::Mantle => "mantle".to_owned(),
        ChainName::Gnosis => "xdai".to_owned(),
        ChainName::Fantom => "fantom".to_owned(),
        ChainName::Celo => "celo".to_owned(),
        ChainName::Core => "core".to_owned(),
        ChainName::Manta => "".to_owned(),
        ChainName::Taiko => "".to_owned(),
        ChainName::Nova => "arbitrum-nova".to_owned(),
    }
}