use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, Instant};
use anyhow::Result;
use colored::Colorize;
use anchor_client::solana_sdk::signature::Signature;
use anchor_client::solana_sdk::signer::Signer;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    common::{config::AppState, logger::Logger},
    dex::raydium_cpmm::RaydiumCPMM,
    engine::swap::{SwapDirection, SwapInType},
    common::config::SwapConfig,
};

#[derive(Clone)]
pub struct RandomTrader {
    app_state: Arc<AppState>,
    raydium_cpmm: RaydiumCPMM,
    target_mint: String,
    logger: Logger,
    is_running: Arc<tokio::sync::RwLock<bool>>,
    progressive_buy_count: Arc<tokio::sync::RwLock<u32>>,
    progressive_sell_count: Arc<tokio::sync::RwLock<u32>>,
    counter: Arc<AtomicU64>, // For deterministic "randomness"
}

#[derive(Debug, Clone)]
pub struct RandomTraderConfig {
    pub min_buy_amount: f64,
    pub max_buy_amount: f64,
    pub min_sell_percentage: f64,
    pub max_sell_percentage: f64,
    pub min_interval_seconds: u64,
    pub max_interval_seconds: u64,
    pub progressive_increase_factor: f64,
    pub max_progressive_steps: u32,
}

impl Default for RandomTraderConfig {
    fn default() -> Self {
        Self {
            min_buy_amount: 0.001,      // 0.001 SOL minimum
            max_buy_amount: 0.01,       // 0.01 SOL maximum
            min_sell_percentage: 0.1,   // 10% minimum
            max_sell_percentage: 0.5,   // 50% maximum
            min_interval_seconds: 30,   // 30 seconds minimum
            max_interval_seconds: 300,  // 5 minutes maximum
            progressive_increase_factor: 1.2, // 20% increase each step
            max_progressive_steps: 10,  // Maximum 10 progressive steps
        }
    }
}

impl RandomTrader {
    pub fn new(app_state: Arc<AppState>, target_mint: String) -> Self {
        let raydium_cpmm = RaydiumCPMM::new(
            app_state.wallet.clone(),
            Some(app_state.rpc_client.clone()),
            Some(app_state.rpc_nonblocking_client.clone()),
        );
        
        Self {
            app_state,
            raydium_cpmm,
            target_mint,
            logger: Logger::new("[RANDOM-TRADER] => ".magenta().to_string()),
            is_running: Arc::new(tokio::sync::RwLock::new(false)),
            progressive_buy_count: Arc::new(tokio::sync::RwLock::new(0)),
            progressive_sell_count: Arc::new(tokio::sync::RwLock::new(0)),
            counter: Arc::new(AtomicU64::new(0)),
        }
    }
    
    /// Generate pseudo-random number using atomic counter
    fn next_pseudo_random(&self) -> u64 {
        let counter = self.counter.fetch_add(1, Ordering::SeqCst);
        // Simple linear congruential generator
        (counter.wrapping_mul(1103515245).wrapping_add(12345)) & 0x7fffffff
    }
    
    /// Generate random value in range using pseudo-random
    fn random_in_range(&self, min: u64, max: u64) -> u64 {
        if min >= max {
            return min;
        }
        let range = max - min;
        let random = self.next_pseudo_random();
        min + (random % range)
    }
    
    /// Generate random float in range
    fn random_float_in_range(&self, min: f64, max: f64) -> f64 {
        if min >= max {
            return min;
        }
        let random = self.next_pseudo_random() as f64 / (0x7fffffff as f64);
        min + (max - min) * random
    }
    
    /// Start the random trading engine
    pub async fn start(&self, config: RandomTraderConfig) -> Result<()> {
        {
            let mut running = self.is_running.write().await;
            if *running {
                return Err(anyhow::anyhow!("Random trader is already running"));
            }
            *running = true;
        }
        
        self.logger.log("Starting random trading engine...".green().to_string());
        self.logger.log(format!("Target mint: {}", self.target_mint));
        self.logger.log(format!("Config: {:?}", config));
        
        // Spawn buy and sell tasks concurrently
        let buy_task = self.spawn_random_buy_task(config.clone());
        let sell_task = self.spawn_random_sell_task(config.clone());
        
        // Wait for both tasks (they should run indefinitely)
        tokio::try_join!(buy_task, sell_task)?;
        
        Ok(())
    }
    
    /// Stop the random trading engine
    pub async fn stop(&self) {
        let mut running = self.is_running.write().await;
        *running = false;
        self.logger.log("Random trading engine stopped".red().to_string());
    }
    
    /// Check if the trader is running
    pub async fn is_running(&self) -> bool {
        *self.is_running.read().await
    }
    
    /// Spawn random buy task
    async fn spawn_random_buy_task(&self, config: RandomTraderConfig) -> Result<()> {
        while self.is_running().await {
            // Generate random interval
            let interval = self.random_in_range(config.min_interval_seconds, config.max_interval_seconds);
            self.logger.log(format!("Next buy in {} seconds", interval).yellow().to_string());
            sleep(Duration::from_secs(interval)).await;
            
            if !self.is_running().await {
                break;
            }
            
            // Execute random buy
            if let Err(e) = self.execute_random_buy(&config).await {
                self.logger.log(format!("Random buy failed: {}", e).red().to_string());
                // Continue even if buy fails
            }
        }
        
        Ok(())
    }
    
    /// Spawn random sell task
    async fn spawn_random_sell_task(&self, config: RandomTraderConfig) -> Result<()> {
        while self.is_running().await {
            // Generate random interval
            let interval = self.random_in_range(config.min_interval_seconds, config.max_interval_seconds);
            self.logger.log(format!("Next sell in {} seconds", interval).blue().to_string());
            sleep(Duration::from_secs(interval)).await;
            
            if !self.is_running().await {
                break;
            }
            
            // Execute random sell
            if let Err(e) = self.execute_random_sell(&config).await {
                self.logger.log(format!("Random sell failed: {}", e).red().to_string());
                // Continue even if sell fails
            }
        }
        
        Ok(())
    }
    
    /// Execute a random buy with progressive amounts
    async fn execute_random_buy(&self, config: &RandomTraderConfig) -> Result<()> {
        // Get progressive count
        let progressive_count = {
            let count = *self.progressive_buy_count.read().await;
            count.min(config.max_progressive_steps)
        };
        
        // Calculate progressive amount
        let base_amount = self.random_float_in_range(config.min_buy_amount, config.max_buy_amount);
        let progressive_multiplier = config.progressive_increase_factor.powi(progressive_count as i32);
        let buy_amount = base_amount * progressive_multiplier;
        
        self.logger.log(format!(
            "Executing random buy - Amount: {} SOL, Progressive step: {}/{}",
            buy_amount, progressive_count + 1, config.max_progressive_steps
        ).green().to_string());
        
        // Create swap config for buy
        let swap_config = SwapConfig {
            mint: self.target_mint.clone(),
            swap_direction: SwapDirection::Buy,
            in_type: SwapInType::Qty,
            amount_in: buy_amount,
            slippage: 1000, // 10% slippage
            max_buy_amount: buy_amount,
        };
        
        // Execute the swap
        let start_time = Instant::now();
        match self.raydium_cpmm.build_swap_from_default_info(swap_config).await {
            Ok((keypair, instructions, token_price)) => {
                self.logger.log(format!("Token price: ${:.8}", token_price));
                
                // Send transaction
                match self.send_swap_transaction(&keypair, instructions).await {
                    Ok(signature) => {
                        self.logger.log(format!(
                            "✅ Random buy successful! Amount: {} SOL, Signature: {}, Time: {:?}",
                            buy_amount, signature, start_time.elapsed()
                        ).green().bold().to_string());
                        
                        // Increment progressive count
                        {
                            let mut count = self.progressive_buy_count.write().await;
                            *count += 1;
                        }
                    },
                    Err(e) => {
                        self.logger.log(format!("❌ Random buy transaction failed: {}", e).red().to_string());
                        return Err(e);
                    }
                }
            },
            Err(e) => {
                self.logger.log(format!("❌ Random buy preparation failed: {}", e).red().to_string());
                return Err(e);
            }
        }
        
        Ok(())
    }
    
    /// Execute a random sell with progressive amounts
    async fn execute_random_sell(&self, config: &RandomTraderConfig) -> Result<()> {
        // Get progressive count
        let progressive_count = {
            let count = *self.progressive_sell_count.read().await;
            count.min(config.max_progressive_steps)
        };
        
        // Calculate progressive percentage
        let base_percentage = self.random_float_in_range(config.min_sell_percentage, config.max_sell_percentage);
        let progressive_multiplier = config.progressive_increase_factor.powi(progressive_count as i32);
        let sell_percentage = (base_percentage * progressive_multiplier).min(1.0); // Cap at 100%
        
        self.logger.log(format!(
            "Executing random sell - Percentage: {:.1}%, Progressive step: {}/{}",
            sell_percentage * 100.0, progressive_count + 1, config.max_progressive_steps
        ).blue().to_string());
        
        // Create swap config for sell
        let swap_config = SwapConfig {
            mint: self.target_mint.clone(),
            swap_direction: SwapDirection::Sell,
            in_type: SwapInType::Pct,
            amount_in: sell_percentage,
            slippage: 1000, // 10% slippage
            max_buy_amount: 0.0, // Not used for sells
        };
        
        // Execute the swap
        let start_time = Instant::now();
        match self.raydium_cpmm.build_swap_from_default_info(swap_config).await {
            Ok((keypair, instructions, token_price)) => {
                self.logger.log(format!("Token price: ${:.8}", token_price));
                
                // Send transaction
                match self.send_swap_transaction(&keypair, instructions).await {
                    Ok(signature) => {
                        self.logger.log(format!(
                            "✅ Random sell successful! Percentage: {:.1}%, Signature: {}, Time: {:?}",
                            sell_percentage * 100.0, signature, start_time.elapsed()
                        ).blue().bold().to_string());
                        
                        // Increment progressive count
                        {
                            let mut count = self.progressive_sell_count.write().await;
                            *count += 1;
                        }
                    },
                    Err(e) => {
                        self.logger.log(format!("❌ Random sell transaction failed: {}", e).red().to_string());
                        return Err(e);
                    }
                }
            },
            Err(e) => {
                self.logger.log(format!("❌ Random sell preparation failed: {}", e).red().to_string());
                return Err(e);
            }
        }
        
        Ok(())
    }
    
    /// Send swap transaction to the network
    async fn send_swap_transaction(
        &self,
        keypair: &Arc<anchor_client::solana_sdk::signature::Keypair>,
        instructions: Vec<anchor_client::solana_sdk::instruction::Instruction>,
    ) -> Result<Signature> {
        use anchor_client::solana_sdk::transaction::Transaction;
        
        // Get recent blockhash
        let recent_blockhash = self.app_state.rpc_client
            .get_latest_blockhash()
            .map_err(|e| anyhow::anyhow!("Failed to get recent blockhash: {}", e))?;
        
        // Create and sign transaction
        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&keypair.pubkey()),
            &[keypair.as_ref()],
            recent_blockhash,
        );
        
        // Send transaction
        let signature = self.app_state.rpc_client
            .send_and_confirm_transaction(&transaction)
            .map_err(|e| anyhow::anyhow!("Failed to send transaction: {}", e))?;
        
        Ok(signature)
    }
    
    /// Reset progressive counters
    pub async fn reset_progressive_counters(&self) {
        {
            let mut buy_count = self.progressive_buy_count.write().await;
            *buy_count = 0;
        }
        {
            let mut sell_count = self.progressive_sell_count.write().await;
            *sell_count = 0;
        }
        self.logger.log("Progressive counters reset".yellow().to_string());
    }
    
    /// Get current progressive counts
    pub async fn get_progressive_counts(&self) -> (u32, u32) {
        let buy_count = *self.progressive_buy_count.read().await;
        let sell_count = *self.progressive_sell_count.read().await;
        (buy_count, sell_count)
    }
} 