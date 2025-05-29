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

const COLLECTION_INTERVAL_SECS: u64 = 1800; // 30 minutes
const POLL_INTERVAL_SECS: u64 = 30;
const SCAN_DURATION_SECS: u64 = 20;

async fn main_loop(config: SensorConfig) -> Result<(), Box<dyn std::error::Error>> {
    info!("Starting RuuviTag data collection service");

    loop {
        let mut measurements: HashMap<String, Vec<RuuviData>> = HashMap::new();
        let start_time = OffsetDateTime::now_utc();

        info!(
            "Starting collection interval at: {}",
            format_datetime(&start_time)
        );

        // Collect data for COLLECTION_INTERVAL_SECS
        loop {
            let elapsed = duration_to_seconds(OffsetDateTime::now_utc() - start_time);
            if elapsed >= COLLECTION_INTERVAL_SECS {
                break;
            }

            let current_data = match scan_for_ruuvitags(&config).await {
                Ok(data) => data,
                Err(e) => {
                    error!("Scan failed: {}", e);
                    continue;
                }
            };

            // Store received data
            for (sensor_id, sensor_data) in current_data {
                measurements
                    .entry(sensor_id)
                    .or_insert_with(Vec::new)
                    .push(sensor_data);
            }

            // Calculate remaining time in interval
            let time_elapsed = duration_to_seconds(OffsetDateTime::now_utc() - start_time);
            let time_remaining = COLLECTION_INTERVAL_SECS.saturating_sub(time_elapsed);

            if time_remaining == 0 {
                break;
            }

            // Wait until next poll time, but not longer than what's remaining
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

        // Calculate averages for all sensors
        let sensor_averages = calculate_averages(&measurements, &config);

        // Store data in both tables
        for (sensor_id, avg_data) in sensor_averages.iter() {
            // Store atmospheric data
            if let Err(e) = store_sensor_data(sensor_id, avg_data, &config.database_url).await {
                error!(
                    "Failed to store sensor data for sensor {}: {}",
                    sensor_id, e
                );
            } else {
                info!("Successfully stored sensor data for sensor {}", sensor_id);
            }

            // Store movement data
            if let Err(e) = store_movement_data(sensor_id, avg_data, &config.database_url).await {
                error!(
                    "Failed to store movement data for sensor {}: {}",
                    sensor_id, e
                );
            } else {
                info!("Successfully stored movement data for sensor {}", sensor_id);
            }
        }

        // Print summary
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
