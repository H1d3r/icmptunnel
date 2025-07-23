use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use anchor_client::solana_sdk::signature::Keypair;
use anchor_client::solana_sdk::signer::Signer;
use colored::Colorize;
use rand::seq::SliceRandom;
use rand::Rng;
use crate::common::logger::Logger;

/// Wallet profile types that determine trading behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WalletProfile {
    FrequentSeller,   // Sells often, shorter hold times
    LongTermHolder,   // Holds for long periods, rarely sells
    BalancedTrader,   // Balanced buy/sell behavior
    Aggressive,       // More frequent trading, higher amounts
    Conservative,     // Less frequent trading, smaller amounts
}

impl WalletProfile {
    /// Get the sell probability for this wallet profile
    pub fn get_sell_probability(&self) -> f64 {
        match self {
            WalletProfile::FrequentSeller => 0.45,  // 45% chance of selling
            WalletProfile::LongTermHolder => 0.15,  // 15% chance of selling
            WalletProfile::BalancedTrader => 0.30,  // 30% chance of selling
            WalletProfile::Aggressive => 0.35,      // 35% chance of selling
            WalletProfile::Conservative => 0.25,    // 25% chance of selling
        }
    }
    
    /// Get the minimum hold time in hours for this wallet profile
    pub fn get_min_hold_time_hours(&self) -> u64 {
        match self {
            WalletProfile::FrequentSeller => 6,   // 6 hours minimum
            WalletProfile::LongTermHolder => 72,  // 72 hours minimum (3 days)
            WalletProfile::BalancedTrader => 24,  // 24 hours minimum
            WalletProfile::Aggressive => 4,       // 4 hours minimum
            WalletProfile::Conservative => 48,    // 48 hours minimum (2 days)
        }
    }
    
    /// Get the maximum hold time in hours for this wallet profile
    pub fn get_max_hold_time_hours(&self) -> u64 {
        match self {
            WalletProfile::FrequentSeller => 48,   // 48 hours maximum (2 days)
            WalletProfile::LongTermHolder => 168,  // 168 hours maximum (7 days)
            WalletProfile::BalancedTrader => 96,   // 96 hours maximum (4 days)
            WalletProfile::Aggressive => 24,       // 24 hours maximum
            WalletProfile::Conservative => 120,    // 120 hours maximum (5 days)
        }
    }
    
    /// Get the trading amount multiplier for this wallet profile
    pub fn get_amount_multiplier(&self) -> f64 {
        match self {
            WalletProfile::FrequentSeller => 0.8,  // 80% of base amount
            WalletProfile::LongTermHolder => 1.2,  // 120% of base amount
            WalletProfile::BalancedTrader => 1.0,  // 100% of base amount
            WalletProfile::Aggressive => 1.5,      // 150% of base amount
            WalletProfile::Conservative => 0.6,    // 60% of base amount
        }
    }
    
    /// Get the trading frequency multiplier for this wallet profile
    pub fn get_frequency_multiplier(&self) -> f64 {
        match self {
            WalletProfile::FrequentSeller => 0.7,  // 70% of base interval (more frequent)
            WalletProfile::LongTermHolder => 2.0,  // 200% of base interval (less frequent)
            WalletProfile::BalancedTrader => 1.0,  // 100% of base interval
            WalletProfile::Aggressive => 0.5,      // 50% of base interval (more frequent)
            WalletProfile::Conservative => 1.5,    // 150% of base interval (less frequent)
        }
    }
    
    /// Randomly assign a wallet profile based on realistic distribution
    pub fn random_profile() -> Self {
        let mut rng = rand::thread_rng();
        let random_value = rng.gen::<f64>();
        
        match random_value {
            x if x < 0.20 => WalletProfile::FrequentSeller,  // 20%
            x if x < 0.35 => WalletProfile::LongTermHolder,  // 15%
            x if x < 0.70 => WalletProfile::BalancedTrader,  // 35%
            x if x < 0.85 => WalletProfile::Aggressive,      // 15%
            _ => WalletProfile::Conservative,                 // 15%
        }
    }
}

/// Wallet information including profile and trading history
#[derive(Debug, Clone)]
pub struct WalletInfo {
    pub keypair: Arc<Keypair>,
    pub profile: WalletProfile,
    pub usage_count: u32,
    pub last_buy_time: Option<tokio::time::Instant>,
    pub last_sell_time: Option<tokio::time::Instant>,
    pub total_buys: u32,
    pub total_sells: u32,
    pub created_at: tokio::time::Instant,
}

impl WalletInfo {
    pub fn new(keypair: Arc<Keypair>) -> Self {
        Self {
            keypair,
            profile: WalletProfile::random_profile(),
            usage_count: 0,
            last_buy_time: None,
            last_sell_time: None,
            total_buys: 0,
            total_sells: 0,
            created_at: tokio::time::Instant::now(),
        }
    }
    
    pub fn pubkey(&self) -> anchor_client::solana_sdk::pubkey::Pubkey {
        self.keypair.pubkey()
    }
    
    /// Check if this wallet can sell based on its profile and hold time
    pub fn can_sell(&self, min_global_delay_hours: u64, max_global_delay_hours: u64) -> bool {
        let profile_min_delay = self.profile.get_min_hold_time_hours();
        let profile_max_delay = self.profile.get_max_hold_time_hours();
        
        // Use the stricter of profile or global delay
        let min_delay = profile_min_delay.max(min_global_delay_hours);
        let max_delay = profile_max_delay.min(max_global_delay_hours);
        
        if let Some(last_buy) = self.last_buy_time {
            let hours_since_buy = last_buy.elapsed().as_secs() / 3600;
            
            // Generate random delay between min and max
            let mut rng = rand::thread_rng();
            let required_delay = min_delay + rng.gen_range(0..=(max_delay - min_delay));
            
            hours_since_buy >= required_delay
        } else {
            // No previous buy, can't sell
            false
        }
    }
    
    /// Update buy statistics
    pub fn record_buy(&mut self) {
        self.usage_count += 1;
        self.total_buys += 1;
        self.last_buy_time = Some(tokio::time::Instant::now());
    }
    
    /// Update sell statistics
    pub fn record_sell(&mut self) {
        self.usage_count += 1;
        self.total_sells += 1;
        self.last_sell_time = Some(tokio::time::Instant::now());
    }
}

/// Wallet pool for managing multiple wallets with sophisticated randomization
pub struct WalletPool {
    wallets: Vec<WalletInfo>,
    logger: Logger,
}

impl WalletPool {
    /// Create a new wallet pool and load all wallets from the /wallet directory
    pub fn new() -> Result<Self, String> {
        let logger = Logger::new("[WALLET-POOL] => ".cyan().bold().to_string());
        
        logger.log("🔑 Initializing wallet pool with profiles...".cyan().to_string());
        
        let wallet_dir = "./wallet";
        if !Path::new(wallet_dir).exists() {
            return Err(format!("Wallet directory not found: {}", wallet_dir));
        }
        
        // Read all wallet files
        let entries = fs::read_dir(wallet_dir)
            .map_err(|e| format!("Failed to read wallet directory: {}", e))?;
        
        let mut wallets = Vec::new();
        
        for entry in entries {
            let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
            let path = entry.path();
            
            if path.is_file() && path.extension().map_or(false, |ext| ext == "txt") {
                match Self::load_wallet_from_file(&path) {
                    Ok(keypair) => {
                        let wallet_info = WalletInfo::new(Arc::new(keypair));
                        
                        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                            logger.log(format!("✅ Loaded wallet: {} -> {} (Profile: {:?})", 
                                filename, wallet_info.pubkey(), wallet_info.profile));
                        }
                        
                        wallets.push(wallet_info);
                    },
                    Err(e) => {
                        logger.log(format!("❌ Failed to load wallet from {:?}: {}", path, e).red().to_string());
                    }
                }
            }
        }
        
        if wallets.is_empty() {
            return Err("No valid wallets found in the wallet directory".to_string());
        }
        
        // Log profile distribution
        let mut profile_counts = HashMap::new();
        for wallet in &wallets {
            *profile_counts.entry(wallet.profile).or_insert(0) += 1;
        }
        
        logger.log(format!("🎯 Wallet pool initialized with {} wallets", wallets.len()).green().bold().to_string());
        for (profile, count) in profile_counts {
            logger.log(format!("   {:?}: {} wallets", profile, count));
        }
        
        Ok(Self {
            wallets,
            logger,
        })
    }
    
    /// Load a single wallet from a file
    fn load_wallet_from_file(path: &Path) -> Result<Keypair, String> {
        let private_key = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read wallet file: {}", e))?
            .trim()
            .to_string();
        
        if private_key.len() < 85 {
            return Err(format!("Invalid private key length: {}", private_key.len()));
        }
        
        let keypair = Keypair::from_base58_string(&private_key);
        Ok(keypair)
    }
    
    /// Get a random wallet with sophisticated selection algorithm
    /// This implements weighted random selection favoring less-used wallets
    pub fn get_random_wallet(&mut self) -> Arc<Keypair> {
        let mut rng = rand::thread_rng();
        
        // Calculate weights inversely proportional to usage count
        let max_usage = self.wallets.iter().map(|w| w.usage_count).max().unwrap_or(0);
        let min_usage = self.wallets.iter().map(|w| w.usage_count).min().unwrap_or(0);
        
        // If all wallets have been used equally, just pick randomly
        if max_usage == min_usage {
            let selected_idx = rng.gen_range(0..self.wallets.len());
            self.wallets[selected_idx].usage_count += 1;
            return self.wallets[selected_idx].keypair.clone();
        }
        
        // Weighted selection favoring less-used wallets
        let weights: Vec<f64> = self.wallets.iter().map(|wallet| {
            // Higher weight for less-used wallets
            (max_usage + 1 - wallet.usage_count) as f64
        }).collect();
        
        let total_weight: f64 = weights.iter().sum();
        let mut random_value = rng.gen::<f64>() * total_weight;
        
        for (i, weight) in weights.iter().enumerate() {
            random_value -= weight;
            if random_value <= 0.0 {
                self.wallets[i].usage_count += 1;
                
                self.logger.log(format!("🎯 Selected wallet: {} (Profile: {:?}, Usage: {})", 
                    self.wallets[i].pubkey(), self.wallets[i].profile, self.wallets[i].usage_count));
                
                return self.wallets[i].keypair.clone();
            }
        }
        
        // Fallback to first wallet (shouldn't happen)
        self.wallets[0].usage_count += 1;
        self.wallets[0].keypair.clone()
    }
    
    /// Get a wallet suitable for selling (has bought tokens and can sell based on profile)
    pub fn get_wallet_for_selling(&mut self, min_global_delay_hours: u64, max_global_delay_hours: u64) -> Option<Arc<Keypair>> {
        let mut eligible_wallets: Vec<usize> = self.wallets
            .iter()
            .enumerate()
            .filter(|(_, wallet)| wallet.can_sell(min_global_delay_hours, max_global_delay_hours))
            .map(|(i, _)| i)
            .collect();
        
        if eligible_wallets.is_empty() {
            return None;
        }
        
        // Shuffle to add randomness
        eligible_wallets.shuffle(&mut rand::thread_rng());
        
        // Select the first eligible wallet
        let selected_idx = eligible_wallets[0];
        self.wallets[selected_idx].record_sell();
        
        self.logger.log(format!("🎯 Selected wallet for selling: {} (Profile: {:?})", 
            self.wallets[selected_idx].pubkey(), self.wallets[selected_idx].profile));
        
        Some(self.wallets[selected_idx].keypair.clone())
    }
    
    /// Record a buy transaction for a wallet
    pub fn record_buy_for_wallet(&mut self, wallet_pubkey: &anchor_client::solana_sdk::pubkey::Pubkey) {
        if let Some(wallet) = self.wallets.iter_mut().find(|w| w.pubkey() == *wallet_pubkey) {
            wallet.record_buy();
        }
    }
    
    /// Get wallet count
    pub fn wallet_count(&self) -> usize {
        self.wallets.len()
    }
    
    /// Get wallet usage statistics
    pub fn get_usage_stats(&self) -> HashMap<String, u32> {
        self.wallets.iter()
            .map(|w| (w.pubkey().to_string(), w.usage_count))
            .collect()
    }
    
    /// Get wallet profile statistics
    pub fn get_profile_stats(&self) -> HashMap<WalletProfile, u32> {
        let mut stats = HashMap::new();
        for wallet in &self.wallets {
            *stats.entry(wallet.profile).or_insert(0) += 1;
        }
        stats
    }
    
    /// Reset usage statistics
    pub fn reset_usage_stats(&mut self) {
        for wallet in &mut self.wallets {
            wallet.usage_count = 0;
        }
        self.logger.log("📊 Wallet usage statistics reset".yellow().to_string());
    }
    
    /// Get least used wallets (for balancing)
    pub fn get_least_used_wallets(&self, count: usize) -> Vec<Arc<Keypair>> {
        let mut wallet_pairs: Vec<_> = self.wallets.iter()
            .map(|wallet| (wallet.keypair.clone(), wallet.usage_count))
            .collect();
        
        // Sort by usage count (ascending)
        wallet_pairs.sort_by_key(|(_, usage)| *usage);
        
        wallet_pairs.into_iter()
            .take(count)
            .map(|(keypair, _)| keypair)
            .collect()
    }
    
    /// Generate realistic trading intervals with sophisticated randomization
    pub fn generate_random_interval(&self, base_interval_ms: u64) -> u64 {
        let mut rng = rand::thread_rng();
        
        // Generate intervals that follow a realistic distribution
        // 70% of trades happen within 0.5x to 2x the base interval
        // 20% happen within 2x to 5x the base interval  
        // 10% happen within 5x to 10x the base interval (longer pauses)
        
        let random_factor = rng.gen::<f64>();
        
        let multiplier = if random_factor < 0.7 {
            // 70% - Short intervals (active trading)
            0.5 + rng.gen::<f64>() * 1.5 // 0.5x to 2x
        } else if random_factor < 0.9 {
            // 20% - Medium intervals (normal trading)
            2.0 + rng.gen::<f64>() * 3.0 // 2x to 5x
        } else {
            // 10% - Long intervals (realistic pauses)
            5.0 + rng.gen::<f64>() * 5.0 // 5x to 10x
        };
        
        // Add small random jitter to avoid patterns
        let jitter = 0.9 + rng.gen::<f64>() * 0.2; // 0.9x to 1.1x
        
        ((base_interval_ms as f64 * multiplier * jitter) as u64).max(1000) // Minimum 1 second
    }
    
    /// Generate realistic trading amounts with sophisticated randomization
    pub fn generate_random_amount(&self, min_amount: f64, max_amount: f64) -> f64 {
        let mut rng = rand::thread_rng();
        
        // Generate amounts following a realistic distribution
        // Most trades are smaller amounts, with occasional larger ones
        
        let random_factor = rng.gen::<f64>();
        let range = max_amount - min_amount;
        
        let amount = if random_factor < 0.6 {
            // 60% - Small amounts (bottom 40% of range)
            min_amount + range * 0.4 * rng.gen::<f64>()
        } else if random_factor < 0.9 {
            // 30% - Medium amounts (middle 40% of range)
            min_amount + range * (0.4 + 0.4 * rng.gen::<f64>())
        } else {
            // 10% - Large amounts (top 20% of range)
            min_amount + range * (0.8 + 0.2 * rng.gen::<f64>())
        };
        
        // Round to avoid suspicious precision
        (amount * 1000.0).round() / 1000.0
    }
    
    /// Determine if next trade should be buy or sell with dynamic ratio support
    pub fn should_buy_next(&self, recent_trades: &[TradeType], target_buy_ratio: f64) -> bool {
        let mut rng = rand::thread_rng();
        
        // Count recent trades to maintain overall ratio
        let recent_count = recent_trades.len().min(10); // Look at last 10 trades
        if recent_count == 0 {
            return true; // Start with a buy
        }
        
        let recent_buys = recent_trades.iter()
            .take(recent_count)
            .filter(|&&trade| trade == TradeType::Buy)
            .count();
        
        let buy_ratio = recent_buys as f64 / recent_count as f64;
        
        // Adjust probability based on recent ratio
        let probability = if buy_ratio < target_buy_ratio {
            // Need more buys
            target_buy_ratio + 0.1
        } else if buy_ratio > target_buy_ratio + 0.1 {
            // Need more sells
            target_buy_ratio - 0.2
        } else {
            // Close to target, use base probability
            target_buy_ratio
        };
        
        rng.gen::<f64>() < probability.max(0.0).min(1.0)
    }
}

/// Trade type for tracking recent trades
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TradeType {
    Buy,
    Sell,
}

/// Advanced randomization configuration
#[derive(Debug, Clone)]
pub struct RandomizationConfig {
    pub min_amount_sol: f64,
    pub max_amount_sol: f64,
    pub base_buy_interval_ms: u64,
    pub base_sell_interval_ms: u64,
    pub buy_sell_ratio: f64, // 0.7 = 70% buy, 30% sell
    pub wallet_rotation_frequency: u32, // Change wallet every N trades
    pub enable_realistic_pauses: bool,
    pub max_consecutive_same_wallet: u32,
}

impl Default for RandomizationConfig {
    fn default() -> Self {
        Self {
            min_amount_sol: 0.03,
            max_amount_sol: 0.55,
            base_buy_interval_ms: 600_000,   // 10 minutes base (600 seconds)
            base_sell_interval_ms: 900_000,  // 15 minutes base (900 seconds)
            buy_sell_ratio: 0.7,
            wallet_rotation_frequency: 3, // Change wallet every 3 trades
            enable_realistic_pauses: true,
            max_consecutive_same_wallet: 5,
        }
    }
}

impl RandomizationConfig {
    /// Create a new randomization config with stealth settings
    pub fn stealth_mode() -> Self {
        // Read stealth buy range from environment variables
        let min_stealth_ratio = std::env::var("MIN_STEALTH_BUY_RATIO")
            .unwrap_or_else(|_| "0.5".to_string())
            .parse::<f64>()
            .unwrap_or(0.5);
        
        let max_stealth_ratio = std::env::var("MAX_STEALTH_BUY_RATIO")
            .unwrap_or_else(|_| "0.9".to_string())
            .parse::<f64>()
            .unwrap_or(0.9);
        
        Self {
            min_amount_sol: min_stealth_ratio,
            max_amount_sol: max_stealth_ratio,
            base_buy_interval_ms: 1_200_000,  // 20 minutes base (1200 seconds)
            base_sell_interval_ms: 1_800_000, // 30 minutes base (1800 seconds)
            buy_sell_ratio: 0.7,
            wallet_rotation_frequency: 2, // Change wallet every 2 trades
            enable_realistic_pauses: true,
            max_consecutive_same_wallet: 3,
        }
    }
    
    /// Create a conservative randomization config
    pub fn conservative_mode() -> Self {
        Self {
            min_amount_sol: 0.05,
            max_amount_sol: 0.3,
            base_buy_interval_ms: 30000,  // 30 seconds base
            base_sell_interval_ms: 45000, // 45 seconds base
            buy_sell_ratio: 0.65,
            wallet_rotation_frequency: 5, // Change wallet every 5 trades
            enable_realistic_pauses: true,
            max_consecutive_same_wallet: 7,
        }
    }
} 