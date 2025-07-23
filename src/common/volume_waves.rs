use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant};
use rand::Rng;
use colored::Colorize;
use crate::common::logger::Logger;

/// Volume wave manager that creates realistic trading patterns
pub struct VolumeWaveManager {
    current_phase: TradingPhase,
    phase_start_time: Instant,
    active_duration: Duration,
    slow_duration: Duration,
    logger: Logger,
    activity_multipliers: PhaseMultipliers,
}

impl VolumeWaveManager {
    /// Create a new volume wave manager
    pub fn new(active_hours: u64, slow_hours: u64) -> Self {
        let logger = Logger::new("[VOLUME-WAVES] => ".blue().bold().to_string());
        
        // Start with a random phase
        let mut rng = rand::thread_rng();
        let initial_phase = if rng.gen_bool(0.6) {
            TradingPhase::Active
        } else {
            TradingPhase::Slow
        };
        
        logger.log(format!("🌊 Volume wave manager initialized in {:?} phase", initial_phase).blue().to_string());
        
        Self {
            current_phase: initial_phase,
            phase_start_time: Instant::now(),
            active_duration: Duration::from_secs(active_hours * 3600),
            slow_duration: Duration::from_secs(slow_hours * 3600),
            logger,
            activity_multipliers: PhaseMultipliers::default(),
        }
    }
    
    /// Get the current trading phase, updating if necessary
    pub fn get_current_phase(&mut self) -> TradingPhase {
        let now = Instant::now();
        let elapsed = now.duration_since(self.phase_start_time);
        
        let should_switch = match self.current_phase {
            TradingPhase::Active => elapsed >= self.active_duration,
            TradingPhase::Slow => elapsed >= self.slow_duration,
            TradingPhase::Burst => elapsed >= Duration::from_secs(15 * 60), // Burst lasts 15 minutes
            TradingPhase::Dormant => elapsed >= Duration::from_secs(60 * 60),  // Dormant lasts 1 hour
        };
        
        if should_switch {
            self.switch_phase();
        }
        
        self.current_phase
    }
    
    /// Switch to the next trading phase
    fn switch_phase(&mut self) {
        let old_phase = self.current_phase;
        let mut rng = rand::thread_rng();
        
        self.current_phase = match self.current_phase {
            TradingPhase::Active => {
                // After active, go to slow with occasional burst
                if rng.gen_bool(0.15) { // 15% chance of burst
                    TradingPhase::Burst
                } else {
                    TradingPhase::Slow
                }
            },
            TradingPhase::Slow => {
                // After slow, go to active with occasional dormant
                if rng.gen_bool(0.1) { // 10% chance of dormant
                    TradingPhase::Dormant
                } else {
                    TradingPhase::Active
                }
            },
            TradingPhase::Burst => {
                // After burst, always go to slow to cool down
                TradingPhase::Slow
            },
            TradingPhase::Dormant => {
                // After dormant, always go to active
                TradingPhase::Active
            },
        };
        
        self.phase_start_time = Instant::now();
        
        let duration_text = match self.current_phase {
            TradingPhase::Active => format!("{:.1} hours", self.active_duration.as_secs_f64() / 3600.0),
            TradingPhase::Slow => format!("{:.1} hours", self.slow_duration.as_secs_f64() / 3600.0),
            TradingPhase::Burst => "15 minutes".to_string(),
            TradingPhase::Dormant => "1 hour".to_string(),
        };
        
        self.logger.log(format!(
            "🔄 Phase transition: {:?} -> {:?} (Duration: {})",
            old_phase, self.current_phase, duration_text
        ).blue().bold().to_string());
    }
    
    /// Get the frequency multiplier for the current phase
    pub fn get_frequency_multiplier(&self) -> f64 {
        match self.current_phase {
            TradingPhase::Active => self.activity_multipliers.active_frequency,
            TradingPhase::Slow => self.activity_multipliers.slow_frequency,
            TradingPhase::Burst => self.activity_multipliers.burst_frequency,
            TradingPhase::Dormant => self.activity_multipliers.dormant_frequency,
        }
    }
    
    /// Get the amount multiplier for the current phase
    pub fn get_amount_multiplier(&self) -> f64 {
        match self.current_phase {
            TradingPhase::Active => self.activity_multipliers.active_amount,
            TradingPhase::Slow => self.activity_multipliers.slow_amount,
            TradingPhase::Burst => self.activity_multipliers.burst_amount,
            TradingPhase::Dormant => self.activity_multipliers.dormant_amount,
        }
    }
    
    /// Get comprehensive wave information
    pub fn get_wave_info(&self) -> VolumeWaveInfo {
        let elapsed = Instant::now().duration_since(self.phase_start_time);
        let remaining = match self.current_phase {
            TradingPhase::Active => self.active_duration.saturating_sub(elapsed),
            TradingPhase::Slow => self.slow_duration.saturating_sub(elapsed),
            TradingPhase::Burst => Duration::from_secs(15 * 60).saturating_sub(elapsed),
            TradingPhase::Dormant => Duration::from_secs(60 * 60).saturating_sub(elapsed),
        };
        
        VolumeWaveInfo {
            current_phase: self.current_phase,
            time_in_phase: elapsed,
            time_remaining: remaining,
            frequency_multiplier: self.get_frequency_multiplier(),
            amount_multiplier: self.get_amount_multiplier(),
        }
    }
    
    /// Set custom activity multipliers
    pub fn set_activity_multipliers(&mut self, multipliers: PhaseMultipliers) {
        self.activity_multipliers = multipliers;
        self.logger.log("⚙️ Activity multipliers updated".yellow().to_string());
    }
    
    /// Force a specific phase (for testing)
    pub fn force_phase(&mut self, phase: TradingPhase) {
        let old_phase = self.current_phase;
        self.current_phase = phase;
        self.phase_start_time = Instant::now();
        
        self.logger.log(format!(
            "🔧 Phase forced: {:?} -> {:?}",
            old_phase, self.current_phase
        ).yellow().to_string());
    }
    
    /// Get natural wave intervals that vary throughout the day
    pub fn get_natural_interval(&self, base_interval_ms: u64) -> u64 {
        let mut rng = rand::thread_rng();
        
        // Base multiplier from phase
        let phase_multiplier = self.get_frequency_multiplier();
        
        // Add time-based variation (simulate daily patterns)
        let hour_of_day = (Instant::now().duration_since(self.phase_start_time).as_secs() / 3600) % 24;
        let time_multiplier = match hour_of_day {
            6..=9 => 0.8,    // Morning active
            10..=11 => 1.2,  // Late morning slow
            12..=14 => 0.9,  // Lunch active
            15..=17 => 0.7,  // Afternoon active
            18..=20 => 1.1,  // Evening slow
            21..=23 => 1.3,  // Night slow
            0..=5 => 1.5,    // Very early morning slow
            _ => 1.0,
        };
        
        // Add random variation (±20%)
        let random_variation = 0.8 + rng.gen::<f64>() * 0.4;
        
        let final_multiplier = phase_multiplier * time_multiplier * random_variation;
        
        (base_interval_ms as f64 * final_multiplier) as u64
    }
}

/// Trading phases for volume waves
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TradingPhase {
    Active,   // Normal high activity
    Slow,     // Reduced activity
    Burst,    // Very high activity (short duration)
    Dormant,  // Minimal activity (longer pause)
}

/// Activity multipliers for different phases
#[derive(Debug, Clone)]
pub struct PhaseMultipliers {
    pub active_frequency: f64,   // Frequency multiplier for active phase
    pub active_amount: f64,      // Amount multiplier for active phase
    pub slow_frequency: f64,     // Frequency multiplier for slow phase
    pub slow_amount: f64,        // Amount multiplier for slow phase
    pub burst_frequency: f64,    // Frequency multiplier for burst phase
    pub burst_amount: f64,       // Amount multiplier for burst phase
    pub dormant_frequency: f64,  // Frequency multiplier for dormant phase
    pub dormant_amount: f64,     // Amount multiplier for dormant phase
}

impl Default for PhaseMultipliers {
    fn default() -> Self {
        Self {
            active_frequency: 1.0,    // Normal speed
            active_amount: 1.0,       // Normal amounts
            slow_frequency: 2.5,      // 2.5x slower (longer intervals)
            slow_amount: 0.7,         // 70% of normal amounts
            burst_frequency: 0.3,     // 3x faster (shorter intervals)
            burst_amount: 1.5,        // 150% of normal amounts
            dormant_frequency: 4.0,   // 4x slower (very long intervals)
            dormant_amount: 0.3,      // 30% of normal amounts
        }
    }
}

/// Information about current volume wave state
#[derive(Debug, Clone)]
pub struct VolumeWaveInfo {
    pub current_phase: TradingPhase,
    pub time_in_phase: Duration,
    pub time_remaining: Duration,
    pub frequency_multiplier: f64,
    pub amount_multiplier: f64,
}

/// Global volume wave manager instance
pub type GlobalVolumeWaveManager = Arc<Mutex<VolumeWaveManager>>;

/// Create a global volume wave manager
pub fn create_global_volume_wave_manager(active_hours: u64, slow_hours: u64) -> GlobalVolumeWaveManager {
    Arc::new(Mutex::new(VolumeWaveManager::new(active_hours, slow_hours)))
}

/// Organic wave pattern that mimics real market behavior
pub struct OrganicWavePattern {
    volume_manager: VolumeWaveManager,
    daily_cycle_offset: Duration,
    weekly_pattern: WeeklyPattern,
}

impl OrganicWavePattern {
    /// Create a new organic wave pattern
    pub fn new(active_hours: u64, slow_hours: u64) -> Self {
        // Add random daily offset to make patterns less predictable
        let mut rng = rand::thread_rng();
        let offset_hours = rng.gen_range(0..24);
        
        Self {
            volume_manager: VolumeWaveManager::new(active_hours, slow_hours),
            daily_cycle_offset: Duration::from_secs(offset_hours * 60 * 60),
            weekly_pattern: WeeklyPattern::generate_random(),
        }
    }
    
    /// Get interval with organic patterns applied
    pub fn get_organic_interval(&mut self, base_interval_ms: u64) -> u64 {
        let phase = self.volume_manager.get_current_phase();
        let base_with_phase = self.volume_manager.get_natural_interval(base_interval_ms);
        
        // Apply weekly pattern
        let weekly_multiplier = self.weekly_pattern.get_current_multiplier();
        
        (base_with_phase as f64 * weekly_multiplier) as u64
    }
    
    /// Get current phase info
    pub fn get_info(&self) -> VolumeWaveInfo {
        self.volume_manager.get_wave_info()
    }
}

/// Weekly patterns for different days
#[derive(Debug, Clone)]
pub struct WeeklyPattern {
    day_multipliers: [f64; 7], // Monday=0, Sunday=6
}

impl WeeklyPattern {
    /// Generate a random but realistic weekly pattern
    pub fn generate_random() -> Self {
        let mut rng = rand::thread_rng();
        
        // Typical market patterns: weekdays more active, weekends quieter
        let mut multipliers = [0.0; 7];
        
        // Monday-Friday: 0.8-1.2x
        for i in 0..5 {
            multipliers[i] = 0.8 + rng.gen::<f64>() * 0.4;
        }
        
        // Saturday-Sunday: 1.2-1.8x (quieter)
        for i in 5..7 {
            multipliers[i] = 1.2 + rng.gen::<f64>() * 0.6;
        }
        
        Self {
            day_multipliers: multipliers,
        }
    }
    
    /// Get multiplier for current day
    pub fn get_current_multiplier(&self) -> f64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Get day of week (0=Monday, 6=Sunday)
        let day_of_week = ((now / 86400) + 3) % 7; // +3 to adjust for epoch starting on Thursday
        
        self.day_multipliers[day_of_week as usize]
    }
} 