/// Database operations for storing sensor and movement data
use crate::database::connection::execute_with_retry;
use crate::models::AverageData;

/// Store atmospheric sensor data (temperature, humidity, pressure) in database
///
/// This function inserts averaged sensor readings into the sensor_data table.
/// It uses the retry mechanism to handle transient database connection issues.
///
/// # Arguments
/// * `sensor_id` - MAC address of the sensor
/// * `avg_data` - Averaged data to store
/// * `database_url` - PostgreSQL connection string
///
/// # Returns
/// Result indicating success or failure
pub async fn store_sensor_data(
    sensor_id: &str,
    avg_data: &AverageData,
    database_url: &str,
) -> Result<(), String> {
    // Clone data for move into async closure
    let sensor_id = sensor_id.to_string();
    let avg_data = avg_data.clone();

    execute_with_retry(database_url, move |client| {
        let sensor_id = sensor_id.clone();
        let avg_data = avg_data.clone();
        async move {
            // Insert atmospheric data into sensor_data table
            client.execute(
                "INSERT INTO sensor_data(sensor_mac, temperature, humidity, pressure, time, name, samples)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &sensor_id,
                    &avg_data.temperature,
                    &avg_data.humidity,
                    &avg_data.pressure,
                    &avg_data.time,
                    &avg_data.name,
                    &avg_data.samples,
                ],
            ).await
        }
    }).await
}

/// Store movement sensor data (acceleration, movement counter) in database
///
/// This function inserts averaged movement readings into the movement_data table.
/// It uses the retry mechanism to handle transient database connection issues.
///
/// # Arguments
/// * `sensor_id` - MAC address of the sensor
/// * `avg_data` - Averaged data to store
/// * `database_url` - PostgreSQL connection string
///
/// # Returns
/// Result indicating success or failure
pub async fn store_movement_data(
    sensor_id: &str,
    avg_data: &AverageData,
    database_url: &str,
) -> Result<(), String> {
    // Clone data for move into async closure
    let sensor_id = sensor_id.to_string();
    let avg_data = avg_data.clone();

    execute_with_retry(database_url, move |client| {
        let sensor_id = sensor_id.clone();
        let avg_data = avg_data.clone();
        async move {
            // Insert movement data into movement_data table
            client.execute(
                "INSERT INTO movement_data(sensor_mac, acceleration_x, acceleration_y, acceleration_z, movement_counter, time, name, samples)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                &[
                    &sensor_id,
                    &avg_data.acceleration_x,
                    &avg_data.acceleration_y,
                    &avg_data.acceleration_z,
                    &(avg_data.movement_counter as i32),
                    &avg_data.time,
                    &avg_data.name,
                    &avg_data.samples,
                ],
            ).await
        }
    }).await
}
