// ================================================================
// System Architecture Overview
// ================================================================
//
// This RuuviTag sensor data collection system implements an ETL pipeline:
//
// 1. EXTRACT (Bluetooth Module):
//    - Scans for RuuviTag sensors via BLE advertisements
//    - Collects readings over 30-minute intervals
//    - Decodes manufacturer data using RuuviTag format 5 protocol
//    - Handles multiple sensors configured via environment variables
//
// 2. TRANSFORM (Utils Module):
//    - Calculates averages for all sensor metrics
//    - Handles movement counter deltas and data validation
//
// 3. LOAD (Database Module):
//    - Stores atmospheric data (temp, humidity, pressure) in sensor_data table
//    - Stores movement data (acceleration, movement counter) in movement_data table
//    - Implements robust retry logic for transient connection failures
//    - Supports SSL/TLS connections with custom CA certificates
//
// Key Features:
// - Continuous operation with graceful shutdown handling
// - Configurable sensor mapping via environment variables
// - Comprehensive error handling and logging
// - Separation of atmospheric and movement data storage
// - Support for cloud PostgreSQL databases with SSL
//
// Configuration:
// - RUUVI_TAGS: Comma-separated "MAC=Name" pairs for sensor configuration
// - DATABASE_URL: PostgreSQL connection string with SSL parameters
// - Optional .env file support for development
//
// ================================================================
mod bluetooth;
mod config;
mod database;
mod models;
mod utils;

use log::{error, info, warn};
use std::collections::HashMap;
use time::OffsetDateTime;
use tokio::time::{sleep, Duration};

use bluetooth::scanner::scan_for_ruuvitags;
use config::SensorConfig;
use database::operations::{store_movement_data, store_sensor_data};
use models::{AverageData, RuuviData};
use utils::{calculate_averages, duration_to_seconds, format_datetime};

// Configuration constants for data collection timing
const COLLECTION_INTERVAL_SECS: u64 = 1800; // 30 minutes
const POLL_INTERVAL_SECS: u64 = 30;
const SCAN_DURATION_SECS: u64 = 20;

/// Main application loop that continuously collects sensor data
///
/// This function implements the core ETL (Extract, Transform, Load) process:
/// 1. Extract: Scan for RuuviTag sensors over Bluetooth
/// 2. Transform: Calculate averages over collection intervals
/// 3. Load: Store processed data in PostgreSQL database
///
/// The loop runs indefinitely, collecting data in 30-minute intervals.
async fn main_loop(config: SensorConfig) -> Result<(), Box<dyn std::error::Error>> {
    info!("Starting RuuviTag data collection service");

    loop {
        // HashMap to store all measurements during the collection interval
        // Key: sensor MAC address, Value: vector of all readings from that sensor
        let mut measurements: HashMap<String, Vec<RuuviData>> = HashMap::new();
        let start_time = OffsetDateTime::now_utc();

        info!(
            "Starting collection interval at: {}",
            format_datetime(&start_time)
        );

        // Data collection phase - gather readings for COLLECTION_INTERVAL_SECS
        loop {
            let elapsed = duration_to_seconds(OffsetDateTime::now_utc() - start_time);
            if elapsed >= COLLECTION_INTERVAL_SECS {
                break;
            }

            // Perform a single scan for all configured RuuviTags
            let current_data = match scan_for_ruuvitags(&config).await {
                Ok(data) => data,
                Err(e) => {
                    error!("Scan failed: {}", e);
                    continue; // Skip this scan iteration but continue collecting
                }
            };

            // Accumulate data from this scan into our measurements collection
            for (sensor_id, sensor_data) in current_data {
                measurements
                    .entry(sensor_id)
                    .or_insert_with(Vec::new)
                    .push(sensor_data);
            }

            // Calculate how much time is left in the collection interval
            let time_elapsed = duration_to_seconds(OffsetDateTime::now_utc() - start_time);
            let time_remaining = COLLECTION_INTERVAL_SECS.saturating_sub(time_elapsed);

            if time_remaining == 0 {
                break;
            }

            // Wait before next scan, accounting for scan duration
            // Don't wait longer than the remaining collection time
            let sleep_time = std::cmp::min(
                POLL_INTERVAL_SECS.saturating_sub(SCAN_DURATION_SECS),
                time_remaining,
            );

            if sleep_time > 0 {
                sleep(Duration::from_secs(sleep_time)).await;
            }
        }

        let end_time = OffsetDateTime::now_utc();
        info!(
            "Collection interval complete at: {}",
            format_datetime(&end_time)
        );

        // Data processing phase - calculate averages from all collected measurements
        let sensor_averages = calculate_averages(&measurements, &config);

        // Data storage phase - persist averaged data to database
        for (sensor_id, avg_data) in sensor_averages.iter() {
            // Store atmospheric data (temperature, humidity, pressure)
            if let Err(e) = store_sensor_data(sensor_id, avg_data, &config.database_url).await {
                error!(
                    "Failed to store sensor data for sensor {}: {}",
                    sensor_id, e
                );
            } else {
                info!("Successfully stored sensor data for sensor {}", sensor_id);
            }

            // Store movement data (acceleration, movement counter)
            if let Err(e) = store_movement_data(sensor_id, avg_data, &config.database_url).await {
                error!(
                    "Failed to store movement data for sensor {}: {}",
                    sensor_id, e
                );
            } else {
                info!("Successfully stored movement data for sensor {}", sensor_id);
            }
        }

        // Log summary of processed data for monitoring
        for (_, avg_data) in sensor_averages.iter() {
            info!("Summary for {}:", avg_data.name);
            info!("  Average temperature: {:.2}°C", avg_data.temperature);
            info!("  Average humidity: {:.2}%", avg_data.humidity);
            info!("  Average pressure: {:.2} hPa", avg_data.pressure);
            info!("  Average acceleration X: {:.3} g", avg_data.acceleration_x);
            info!("  Average acceleration Y: {:.3} g", avg_data.acceleration_y);
            info!("  Average acceleration Z: {:.3} g", avg_data.acceleration_z);
            info!("  Movement counter delta: {}", avg_data.movement_counter);
            info!("  Based on {} samples", avg_data.samples);
        }

        // Warning if no data collected
        if sensor_averages.is_empty() {
            warn!("No data collected during this interval!");
        }

        // Wait until next interval should start
        let total_elapsed = duration_to_seconds(OffsetDateTime::now_utc() - start_time);
        if total_elapsed < COLLECTION_INTERVAL_SECS {
            let wait_time = COLLECTION_INTERVAL_SECS - total_elapsed;
            info!(
                "Waiting {} seconds until next collection interval",
                wait_time
            );
            sleep(Duration::from_secs(wait_time)).await;
        }
    }
}

/// Application entry point
///
/// Sets up logging, loads configuration, handles graceful shutdown,
/// and starts the main data collection loop.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .format_timestamp_secs()
        .init();

    // Load configuration
    let config = match SensorConfig::new() {
        Ok(config) => config,
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            return Err(e);
        }
    };

    // Handle Ctrl+C gracefully
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for Ctrl+C");
        let _ = tx.send(());
    });

    // Run main loop or wait for shutdown signal
    tokio::select! {
        result = main_loop(config) => {
            match result {
                Ok(_) => info!("Program completed successfully"),
                Err(e) => error!("Fatal error: {}", e),
            }
        }
        _ = &mut rx => {
            info!("Program terminated by user. Exiting gracefully.");
        }
    }

    Ok(())
}
