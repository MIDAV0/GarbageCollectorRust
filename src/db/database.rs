use std::fs::File;
use rand::{
    seq::IteratorRandom,
    thread_rng,
};
use serde::{Deserialize, Serialize};

use crate::helpers::utils::read_file_lines;

use super::{
    account::Account,
    constants::{DB_FILE_PATH, PRIVATE_KEYS_FILE_PATH, PROXIES_FILE_PATH},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Database(pub Vec<Account>);

impl Database {
    async fn read_from_file(file_path: &str) -> eyre::Result<Self> {
        let contents = tokio::fs::read_to_string(file_path).await?;
        let db = serde_json::from_str::<Self>(&contents)?;
        Ok(db)
    }

    pub async fn read() -> eyre::Result<Self> {
        Self::read_from_file(DB_FILE_PATH).await
    }

    pub async fn new() -> eyre::Result<Self> {
        let private_keys = read_file_lines(PRIVATE_KEYS_FILE_PATH).await.unwrap();
        let proxies = read_file_lines(PROXIES_FILE_PATH).await.unwrap();
        let mut data = Vec::with_capacity(private_keys.len());

        for (index, pk) in private_keys.iter().enumerate() {
            let proxy = proxies.get(index).cloned();
            let account = Account::new(pk, proxy);
            data.push(account);
        }

        let db_file = File::create(DB_FILE_PATH)?;
        serde_json::to_writer_pretty(db_file, &data)?;

        Ok(Self(data))
    }

    pub fn get_random_account_with_filter<F>(&mut self, filter: F) -> Option<&mut Account>
    where
        F: Fn(&Account) -> bool,
    {
        let mut rng = thread_rng();

        self.0
            .iter_mut()
            .filter(|account| filter(account))
            .choose(&mut rng)
    }

    pub fn update(&self) {
        let file = File::create(DB_FILE_PATH).expect("Default database must be vaild");
        let _ = serde_json::to_writer_pretty(file, &self);
    }
}
