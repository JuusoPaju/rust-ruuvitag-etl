/// Utility functions for data processing and formatting
use std::collections::HashMap;
use time::{format_description, OffsetDateTime};

use crate::config::SensorConfig;
use crate::models::{AverageData, RuuviData};

/// Format a timestamp for human-readable logging
///
/// Converts an OffsetDateTime to DD.MM.YYYY - HH:MM:SS format
/// Falls back to default string representation if formatting fails.
pub fn format_datetime(dt: &OffsetDateTime) -> String {
    let format = format_description::parse("[day].[month].[year] - [hour]:[minute]:[second]")
        .expect("Failed to create format description");
    dt.format(&format).unwrap_or_else(|_| dt.to_string())
}

/// Convert a time::Duration to seconds as u64
///
/// Helper function to work with duration calculations in the main loop.
pub fn duration_to_seconds(duration: time::Duration) -> u64 {
    duration.whole_seconds() as u64
}

/// Calculate average values from collected sensor measurements
///
/// Takes a collection of sensor readings grouped by sensor ID and produces
/// averaged data suitable for database storage. Handles edge cases like
/// empty data sets and wrapping movement counters.
///
/// # Arguments
/// * `measurements` - HashMap mapping sensor MAC addresses to vectors of readings
/// * `config` - Configuration containing sensor name mappings
///
/// # Returns
/// HashMap mapping sensor MAC addresses to calculated averages
pub fn calculate_averages(
    measurements: &HashMap<String, Vec<RuuviData>>,
    config: &SensorConfig,
) -> HashMap<String, AverageData> {
    let mut averages = HashMap::new();

    for (sensor_id, data_points) in measurements {
        // Skip sensors with no data
        if data_points.is_empty() {
            continue;
        }

        let count = data_points.len() as f32;

        // Calculate sums for atmospheric data
        let temp_sum: f32 = data_points.iter().map(|d| d.temperature).sum();
        let humid_sum: f32 = data_points.iter().map(|d| d.humidity).sum();
        let press_sum: f32 = data_points.iter().map(|d| d.pressure).sum();

        // Calculate sums for acceleration data
        let acc_x_sum: f32 = data_points.iter().map(|d| d.acceleration_x).sum();
        let acc_y_sum: f32 = data_points.iter().map(|d| d.acceleration_y).sum();
        let acc_z_sum: f32 = data_points.iter().map(|d| d.acceleration_z).sum();

        // Calculate movement counter delta (handles wrapping)
        // Movement counter increases when the sensor flips
        // We want the total movement during the collection interval
        let movement_delta = data_points
            .last()
            .and_then(|last| {
                data_points
                    .first()
                    .map(|first| last.movement_counter.wrapping_sub(first.movement_counter) as u32)
            })
            .unwrap_or(0);

        // Create averaged data with proper rounding
        let avg_data = AverageData {
            temperature: (temp_sum / count * 100.0).round() / 100.0, // 2 decimal places
            humidity: (humid_sum / count * 100.0).round() / 100.0,   // 2 decimal places
            pressure: (press_sum / count * 100.0).round() / 100.0,   // 2 decimal places
            acceleration_x: (acc_x_sum / count * 1000.0).round() / 1000.0, // 3 decimal places
            acceleration_y: (acc_y_sum / count * 1000.0).round() / 1000.0, // 3 decimal places
            acceleration_z: (acc_z_sum / count * 1000.0).round() / 1000.0, // 3 decimal places
            movement_counter: movement_delta,
            time: OffsetDateTime::now_utc(),
            name: config
                .tags
                .get(sensor_id)
                .cloned()
                .unwrap_or_else(|| "Unknown".to_string()),
            samples: data_points.len() as i32,
        };

        averages.insert(sensor_id.clone(), avg_data);
    }

    averages
}
