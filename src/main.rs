use anyhow::{anyhow, Result};
use config::Config;
use log::{error, info};
use solana_client::rpc_client::RpcClient;
use solana_program::system_instruction;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use std::str::FromStr;
use std::time::Duration;

#[derive(Debug, serde_derive::Deserialize)]
struct Settings {
    network: NetworkConfig,
    keys: KeysConfig,
    transaction: TransactionConfig,
}

#[derive(Debug, serde_derive::Deserialize)]
struct NetworkConfig {
    rpc_url: String,
}

#[derive(Debug, serde_derive::Deserialize)]
struct KeysConfig {
    sender_private_key: String,
    receiver_public_key: String,
}

#[derive(Debug, serde_derive::Deserialize)]
struct TransactionConfig {
    amount: u64,
    min_balance: u64,
    confirmation_timeout: u64,
}

struct SolanaTransactionManager {
    config: Settings,
    client: RpcClient,
}

impl SolanaTransactionManager {
    pub fn new(config_path: &str) -> Result<Self> {
        let settings = Self::load_config(config_path)?;
        let client = RpcClient::new_with_timeout(
            settings.network.rpc_url.clone(),
            Duration::from_secs(30),
        );

        Ok(Self {
            config: settings,
            client,
        })
    }

    fn load_config(config_path: &str) -> Result<Settings> {
        let settings = Config::builder()
            .add_source(config::File::with_name(config_path))
            .build()?;

        Ok(settings.try_deserialize()?)
    }

    fn get_balance(&self, pubkey: &Pubkey) -> Result<u64> {
        let balance = self.client.get_balance(pubkey)?;
        Ok(balance)
    }

    fn check_sufficient_balance(&self, sender_pubkey: &Pubkey, amount: u64) -> Result<bool> {
        let balance = self.get_balance(sender_pubkey)?;
        Ok(balance >= amount + self.config.transaction.min_balance)
    }

    pub fn send_transaction(&self) -> Result<String> {
        let sender_keypair = self.create_sender_keypair()?;
        
        let receiver_pubkey = Pubkey::from_str(&self.config.keys.receiver_public_key)
            .map_err(|e| anyhow!("Invalid receiver public key: {}", e))?;

        let current_balance = self.get_balance(&sender_keypair.pubkey())?;
        info!(
            "現在の残高: {} SOL",
            (current_balance as f64) / 1_000_000_000.0
        );

        if !self.check_sufficient_balance(&sender_keypair.pubkey(), self.config.transaction.amount)? {
            return Err(anyhow!(
                "Insufficient balance. Current balance: {} SOL, Required: {} SOL",
                (current_balance as f64) / 1_000_000_000.0,
                ((self.config.transaction.amount + self.config.transaction.min_balance) as f64)
                    / 1_000_000_000.0
            ));
        }

        let instruction = system_instruction::transfer(
            &sender_keypair.pubkey(),
            &receiver_pubkey,
            self.config.transaction.amount,
        );

        let recent_blockhash = self.client.get_latest_blockhash()?;

        let message = Message::new(&[instruction], Some(&sender_keypair.pubkey()));
        let mut transaction = Transaction::new_unsigned(message);
        transaction.sign(&[&sender_keypair], recent_blockhash);

        let signature = self
            .client
            .send_and_confirm_transaction_with_spinner_and_config(
                &transaction,
                CommitmentConfig::confirmed(),
                solana_client::rpc_config::RpcSendTransactionConfig {
                    skip_preflight: true,
                    preflight_commitment: None,
                    encoding: None,
                    max_retries: None,
                    min_context_slot: None,
                },
            )?;

        info!("TX送信成功 - シグネチャ: {}", signature);

        let new_balance = self.get_balance(&sender_keypair.pubkey())?;
        info!(
            "変異後残高: {} SOL",
            (new_balance as f64) / 1_000_000_000.0
        );

        Ok(signature.to_string())
    }

    fn create_sender_keypair(&self) -> Result<Keypair> {
        let private_key = bs58::decode(&self.config.keys.sender_private_key)
            .into_vec()
            .map_err(|e| anyhow!("プライベートキーが違うで: {}", e))?;

        if private_key.len() != 64 {
            return Err(anyhow!("Invalid private key length"));
        }

        let keypair = Keypair::from_bytes(&private_key)
            .map_err(|e| anyhow!("Failed to create keypair: {}", e))?;

        Ok(keypair)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let manager = SolanaTransactionManager::new("config/config.toml")?;

    let sender_keypair = manager.create_sender_keypair()?;
    println!("送信アドレス: {}", sender_keypair.pubkey());
    println!("受取アドレス: {}", manager.config.keys.receiver_public_key);

    let current_balance = manager.get_balance(&sender_keypair.pubkey())?;
    println!(
        "現在の残高: {} SOL",
        (current_balance as f64) / 1_000_000_000.0
    );

    match manager.send_transaction() {
        Ok(signature) => {
            println!("TX成功!: {}", signature);
        }
        Err(e) => {
            error!("Error occurred: {}", e);
        }
    }

    Ok(())
}