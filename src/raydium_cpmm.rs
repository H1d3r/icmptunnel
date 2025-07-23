use std::{str::FromStr, sync::Arc};
use solana_program_pack::Pack;
use anchor_client::solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use anchor_client::solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};
use solana_account_decoder::UiAccountEncoding;
use anyhow::{anyhow, Result};
use colored::Colorize;
use anchor_client::solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Keypair,
    system_program,
    signer::Signer,
};
use spl_token_client::token::TokenError;

use crate::engine::transaction_parser::DexType;
use crate::services::rpc_client;
use spl_associated_token_account::{
    get_associated_token_address,
    instruction::create_associated_token_account_idempotent
};
use spl_token::ui_amount_to_amount;
use tokio::sync::OnceCell;
use lru::LruCache;
use std::num::NonZeroUsize;

use crate::{
    common::{config::SwapConfig, logger::Logger, cache::WALLET_TOKEN_ACCOUNTS},
    core::token,
    engine::swap::{SwapDirection, SwapInType},
};

// Constants - moved to lazy_static for single initialization
lazy_static::lazy_static! {
    static ref TOKEN_PROGRAM: Pubkey = Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap();
    static ref TOKEN_2022_PROGRAM: Pubkey = Pubkey::from_str("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb").unwrap();
    static ref ASSOCIATED_TOKEN_PROGRAM: Pubkey = Pubkey::from_str("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL").unwrap();
    static ref OBSERVATION_STATE: Pubkey = Pubkey::from_str("52z4oFKcZvJ3qcUxujZUhvC5FsWf5m8CGeqL2E9y8T3B").unwrap();
    static ref RAYDIUM_VAULT_AUTHORITY: Pubkey = Pubkey::from_str("GpMZbSM2GgvTKHJirzeGfMFoaZ8UR2X7F4v8vHTvxFbL").unwrap();
    static ref RAYDIUM_CPMM_PROGRAM_ID: Pubkey = Pubkey::from_str("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C").unwrap();
    static ref AMM_CONFIG: Pubkey = Pubkey::from_str("D4FPEruKEHrG5TenZ2mpDGEfu1iUvTiqBxvpU8HLBvC2").unwrap();
    static ref DEFAULT_POOL_QUOTE_ACCOUNT: Pubkey = Pubkey::from_str("H2FkTkXdqjjLMPaAzcmF5FFVAVL1n41QHUUyWmHdmQRN").unwrap();
    static ref DEFAULT_POOL_BASE_ACCOUNT: Pubkey = Pubkey::from_str("Gb3z5zsk3LPNYhXSBLdDjx6kpdxMMT6q6WsU1eKPqtCZ").unwrap();
    static ref DEFAULT_POOL_ID: Pubkey = Pubkey::from_str("51WkKvB7zGPvPd8Hr57xv2rWevVa5CDwVhYQAfFMjTKG").unwrap();
    static ref SOL_MINT: Pubkey = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap();
    static ref SWAP_BASED_INPUT_DISCRIMINATOR: [u8; 8] = [143, 190, 90, 218, 196, 30, 51, 222];
}

// Thread-safe cache with LRU eviction policy
static TOKEN_ACCOUNT_CACHE: OnceCell<LruCache<Pubkey, bool>> = OnceCell::const_new();

const TEN_THOUSAND: u64 = 10000;
const CACHE_SIZE: usize = 1000;

async fn init_caches() {
    TOKEN_ACCOUNT_CACHE.get_or_init(|| async {
        LruCache::new(NonZeroUsize::new(CACHE_SIZE).unwrap())
    }).await;
}

/// A struct to represent the PumpSwap pool which uses constant product AMM
#[derive(Debug, Clone)]
pub struct RaydiumCPMMPool {
    pub pool_id: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub lp_mint: Pubkey,
    pub pool_base_account: Pubkey,
    pub pool_quote_account: Pubkey,
    pub base_reserve: u64,
    pub quote_reserve: u64,
}

#[derive(Clone)]
pub struct RaydiumCPMM {
    pub keypair: Arc<Keypair>,
    pub rpc_client: Option<Arc<anchor_client::solana_client::rpc_client::RpcClient>>,
    pub rpc_nonblocking_client: Option<Arc<anchor_client::solana_client::nonblocking::rpc_client::RpcClient>>,
}


// Optimized instruction creation
fn create_swap_instruction(
    program_id: Pubkey,
    discriminator: [u8; 8],
    base_amount: u64,
    quote_amount: u64,
    accounts: Vec<AccountMeta>,
) -> Instruction {
    let mut data = Vec::with_capacity(24);
    data.extend_from_slice(&discriminator);
    data.extend_from_slice(&base_amount.to_le_bytes());
    data.extend_from_slice(&quote_amount.to_le_bytes());
    
    Instruction { program_id, accounts, data }
}

