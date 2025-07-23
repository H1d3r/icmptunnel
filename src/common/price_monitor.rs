use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant};
use colored::Colorize;
use crate::common::logger::Logger;

/// Price data point for tracking price history
#[derive(Debug, Clone)]
pub struct PricePoint {
    pub price: f64,
    pub timestamp: Instant,
    pub volume_sol: f64,
}

/// Price monitoring system for detecting sharp price movements
pub struct PriceMonitor {
    price_history: VecDeque<PricePoint>,
    logger: Logger,
    max_history_size: usize,
    price_change_threshold: f64,
    throttle_duration: Duration,
    last_throttle_time: Option<Instant>,
    is_throttling: bool,
}

impl PriceMonitor {
    /// Create a new price monitor
    pub fn new(max_history_size: usize, price_change_threshold: f64) -> Self {
        Self {
            price_history: VecDeque::with_capacity(max_history_size),
            logger: Logger::new("[PRICE-MONITOR] => ".magenta().bold().to_string()),
            max_history_size,
            price_change_threshold,
            throttle_duration: Duration::from_secs(30 * 60), // Default 30 minutes throttle
            last_throttle_time: None,
            is_throttling: false,
        }
    }
    
    /// Add a new price point to the monitoring system
    pub fn add_price_point(&mut self, price: f64, volume_sol: f64) {
        let price_point = PricePoint {
            price,
            timestamp: Instant::now(),
            volume_sol,
        };
        
        self.price_history.push_back(price_point);
        
        // Keep only the most recent price points
        while self.price_history.len() > self.max_history_size {
            self.price_history.pop_front();
        }
        
        // Check if we should throttle
        self.check_and_update_throttling();
    }
    
    /// Check if we should throttle trading based on price movement
    fn check_and_update_throttling(&mut self) {
        if self.price_history.len() < 3 {
            return; // Need at least 3 price points
        }
        
        // Calculate price change over the last 10 minutes
        let now = Instant::now();
        let ten_minutes_ago = now - Duration::from_secs(10 * 60);
        
        let recent_prices: Vec<&PricePoint> = self.price_history
            .iter()
            .filter(|p| p.timestamp > ten_minutes_ago)
            .collect();
        
        if recent_prices.len() < 2 {
            return;
        }
        
        let earliest_price = recent_prices.first().unwrap().price;
        let latest_price = recent_prices.last().unwrap().price;
        
        let price_change_pct = (latest_price - earliest_price) / earliest_price;
        
        // Check if price has risen sharply
        if price_change_pct > self.price_change_threshold {
            if !self.is_throttling {
                self.logger.log(format!(
                    "🚨 Price spike detected! Price increased by {:.2}% in 10 minutes. Activating throttling.",
                    price_change_pct * 100.0
                ).red().bold().to_string());
                
                self.is_throttling = true;
                self.last_throttle_time = Some(now);
            }
        } else if self.is_throttling {
            // Check if we should stop throttling
            if let Some(throttle_start) = self.last_throttle_time {
                if now.duration_since(throttle_start) > self.throttle_duration {
                    self.logger.log("✅ Throttling period ended. Resuming normal trading.".green().to_string());
                    self.is_throttling = false;
                    self.last_throttle_time = None;
                }
            }
        }
    }
    
    /// Get the current throttling status
    pub fn is_throttling(&self) -> bool {
        self.is_throttling
    }
    
    /// Get the throttling multiplier (higher value = slower trading)
    pub fn get_throttling_multiplier(&self) -> f64 {
        if self.is_throttling {
            // During throttling, slow down trading by 3x
            3.0
        } else {
            1.0
        }
    }
    
    /// Get the current price trend
    pub fn get_price_trend(&self) -> PriceTrend {
        if self.price_history.len() < 5 {
            return PriceTrend::Neutral;
        }
        
        let recent_prices: Vec<f64> = self.price_history
            .iter()
            .rev()
            .take(5)
            .map(|p| p.price)
            .collect();
        
        let first_price = recent_prices.last().unwrap();
        let last_price = recent_prices.first().unwrap();
        
        let change_pct = (last_price - first_price) / first_price;
        
        if change_pct > 0.05 {
            PriceTrend::StrongUpward
        } else if change_pct > 0.02 {
            PriceTrend::Upward
        } else if change_pct < -0.05 {
            PriceTrend::StrongDownward
        } else if change_pct < -0.02 {
            PriceTrend::Downward
        } else {
            PriceTrend::Neutral
        }
    }
    
    /// Get recent price volatility
    pub fn get_volatility(&self) -> f64 {
        if self.price_history.len() < 3 {
            return 0.0;
        }
        
        let recent_prices: Vec<f64> = self.price_history
            .iter()
            .rev()
            .take(10)
            .map(|p| p.price)
            .collect();
        
        if recent_prices.len() < 2 {
            return 0.0;
        }
        
        let mean = recent_prices.iter().sum::<f64>() / recent_prices.len() as f64;
        let variance = recent_prices.iter()
            .map(|p| (p - mean).powi(2))
            .sum::<f64>() / recent_prices.len() as f64;
        
        variance.sqrt() / mean // Coefficient of variation
    }
    
    /// Get price statistics
    pub fn get_price_stats(&self) -> PriceStats {
        if self.price_history.is_empty() {
            return PriceStats::default();
        }
        
        let prices: Vec<f64> = self.price_history.iter().map(|p| p.price).collect();
        let current_price = prices.last().copied().unwrap_or(0.0);
        let min_price = prices.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        let max_price = prices.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        let avg_price = prices.iter().sum::<f64>() / prices.len() as f64;
        
        PriceStats {
            current_price,
            min_price,
            max_price,
            avg_price,
            volatility: self.get_volatility(),
            trend: self.get_price_trend(),
            data_points: self.price_history.len(),
        }
    }
    
    /// Clear old price data (cleanup)
    pub fn cleanup_old_data(&mut self) {
        let cutoff_time = Instant::now() - Duration::from_secs(24 * 60 * 60);
        
        while let Some(front) = self.price_history.front() {
            if front.timestamp < cutoff_time {
                self.price_history.pop_front();
            } else {
                break;
            }
        }
    }
}

/// Price trend indicators
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PriceTrend {
    StrongUpward,
    Upward,
    Neutral,
    Downward,
    StrongDownward,
}

/// Price statistics summary
#[derive(Debug, Clone)]
pub struct PriceStats {
    pub current_price: f64,
    pub min_price: f64,
    pub max_price: f64,
    pub avg_price: f64,
    pub volatility: f64,
    pub trend: PriceTrend,
    pub data_points: usize,
}

impl Default for PriceStats {
    fn default() -> Self {
        Self {
            current_price: 0.0,
            min_price: 0.0,
            max_price: 0.0,
            avg_price: 0.0,
            volatility: 0.0,
            trend: PriceTrend::Neutral,
            data_points: 0,
        }
    }
}

/// Global price monitor instance
pub type GlobalPriceMonitor = Arc<Mutex<PriceMonitor>>;

/// Create a global price monitor instance
pub fn create_global_price_monitor(price_change_threshold: f64) -> GlobalPriceMonitor {
    Arc::new(Mutex::new(PriceMonitor::new(100, price_change_threshold)))
} 