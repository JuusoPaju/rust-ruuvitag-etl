/// Bluetooth Low Energy scanning and RuuviTag data decoding
use futures_util::StreamExt;
use log::{debug, error, warn};
use std::collections::HashMap;
use tokio::time::{sleep, Duration};

use crate::config::SensorConfig;
use crate::models::RuuviData;

// RuuviTag protocol constants
const RUUVITAG_MANUFACTURER_ID: u16 = 0x0499; // Ruuvi Innovations Ltd. manufacturer ID
const DATA_FORMAT: u8 = 5; // RuuviTag data format version 5
const SCAN_DURATION_SECS: u64 = 20; // How long to actively scan for devices

/// Decode RuuviTag manufacturer data format 5 into structured data
///
/// RuuviTag data format 5 uses a 24-byte payload with the following structure:
/// - Byte 0: Data format (5)
/// - Bytes 1-2: Temperature (signed 16-bit, 0.005°C resolution)
/// - Bytes 3-4: Humidity (unsigned 16-bit, 0.0025% resolution)
/// - Bytes 5-6: Pressure (unsigned 16-bit, +50000 Pa offset, 1 Pa resolution)
/// - Bytes 7-8: Acceleration X (signed 16-bit, 0.001 g resolution)
/// - Bytes 9-10: Acceleration Y (signed 16-bit, 0.001 g resolution)
/// - Bytes 11-12: Acceleration Z (signed 16-bit, 0.001 g resolution)
/// - Bytes 13-14: Battery voltage + TX power (not used here)
/// - Byte 15: Movement counter
/// - Bytes 16-17: Measurement sequence number (not used here)
/// - Bytes 18-23: MAC address (not used here, we get it from BLE)
///
/// # Arguments
/// * `data` - Raw manufacturer data bytes from BLE advertisement
///
/// # Returns
/// Some(RuuviData) if decoding succeeds, None if data is invalid
pub fn decode_ruuvi_data(data: &[u8]) -> Option<RuuviData> {
    if data.len() != 24 || data[0] != DATA_FORMAT {
        if !data.is_empty() {
            warn!(
                "Invalid RuuviTag data: len={}, format={}",
                data.len(),
                data[0]
            );
        }
        return None;
    }

    // Use a closure with error handling for clean code
    match (|| -> Result<RuuviData, Box<dyn std::error::Error>> {
        // Decode temperature: signed 16-bit integer * 0.005°C
        let temperature = i16::from_be_bytes([data[1], data[2]]) as f32 * 0.005;

        // Decode humidity: unsigned 16-bit integer * 0.0025%, capped at 100%
        let humidity = (u16::from_be_bytes([data[3], data[4]]) as f32 * 0.0025).min(100.0);

        // Decode pressure: unsigned 16-bit integer + 50000 Pa, convert to hPa
        let pressure = (u16::from_be_bytes([data[5], data[6]]) as f32 + 50000.0) / 100.0;

        // Decode acceleration values: signed 16-bit integers * 0.001 g
        let acc_x = i16::from_be_bytes([data[7], data[8]]) as f32 * 0.001;
        let acc_y = i16::from_be_bytes([data[9], data[10]]) as f32 * 0.001;
        let acc_z = i16::from_be_bytes([data[11], data[12]]) as f32 * 0.001;

        // Movement counter: increments when significant movement is detected (sensor flips)
        let movement_counter = data[15];

        // Create RuuviData with proper rounding for display
        Ok(RuuviData {
            temperature: (temperature * 100.0).round() / 100.0,
            humidity: (humidity * 100.0).round() / 100.0,
            pressure: (pressure * 100.0).round() / 100.0,
            acceleration_x: (acc_x * 1000.0).round() / 1000.0,
            acceleration_y: (acc_y * 1000.0).round() / 1000.0,
            acceleration_z: (acc_z * 1000.0).round() / 1000.0,
            movement_counter,
        })
    })() {
        Ok(data) => Some(data),
        Err(e) => {
            error!("Error decoding format 5 data: {}", e);
            None
        }
    }
}

/// Scan for configured RuuviTag sensors and collect their data
///
/// This function performs a Bluetooth Low Energy scan to discover configured
/// RuuviTag sensors and decode their advertised data. The scan runs for a
/// fixed duration and returns all valid readings found.
///
/// # Arguments
/// * `config` - Configuration containing sensor MAC addresses to look for
///
/// # Returns
/// Result containing HashMap of sensor MAC -> RuuviData, or error if scan fails
pub async fn scan_for_ruuvitags(
    config: &SensorConfig,
) -> Result<HashMap<String, RuuviData>, Box<dyn std::error::Error>> {
    let mut data = HashMap::new();

    // Initialize Bluetooth session
    let session = match bluer::Session::new().await {
        Ok(session) => session,
        Err(e) => {
            error!("Failed to create Bluetooth session: {}", e);
            return Err(e.into());
        }
    };

    // Get the default Bluetooth adapter
    let adapter = match session.default_adapter().await {
        Ok(adapter) => adapter,
        Err(e) => {
            error!("Failed to get default Bluetooth adapter: {}", e);
            return Err(e.into());
        }
    };

    // Ensure Bluetooth adapter is powered on
    if let Err(e) = adapter.set_powered(true).await {
        error!("Failed to power on adapter: {}", e);
        return Err(e.into());
    }

    // Configure discovery filter for Low Energy devices only
    let filter = bluer::DiscoveryFilter {
        transport: bluer::DiscoveryTransport::Le, // Bluetooth Low Energy only
        duplicate_data: false,                    // Filter out duplicate advertisements
        ..Default::default()
    };

    // Apply the discovery filter (warn if it fails, but continue)
    if let Err(e) = adapter.set_discovery_filter(filter).await {
        warn!("Failed to set discovery filter: {}", e);
    }

    // Start device discovery in background
    let discovery_handle = {
        match adapter.discover_devices().await {
            Ok(discovery_stream) => tokio::spawn(async move {
                let mut stream = discovery_stream;
                while let Some(event) = stream.next().await {
                    debug!("Discovery event: {:?}", event);
                }
            }),
            Err(e) => {
                error!("Failed to start device discovery: {}", e);
                return Err(e.into());
            }
        }
    };

    // Let discovery run for the configured duration
    sleep(Duration::from_secs(SCAN_DURATION_SECS)).await;

    // Stop discovery
    discovery_handle.abort();

    // Get all discovered device addresses
    let devices = match adapter.device_addresses().await {
        Ok(devices) => devices,
        Err(e) => {
            error!("Failed to get device addresses: {}", e);
            return Err(e.into());
        }
    };

    // Process each discovered device
    for addr in devices {
        let device = match adapter.device(addr) {
            Ok(device) => device,
            Err(_) => continue,
        };

        let addr_str = device.address().to_string().to_uppercase();

        // Only process devices that are in our configuration
        if config.tags.contains_key(&addr_str) {
            match device.manufacturer_data().await {
                Ok(Some(manufacturer_data)) => {
                    if let Some(ruuvi_data) = manufacturer_data.get(&RUUVITAG_MANUFACTURER_ID) {
                        // Decode the RuuviTag data
                        if let Some(sensor_data) = decode_ruuvi_data(ruuvi_data) {
                            let log_data = sensor_data.clone();
                            data.insert(addr_str.clone(), sensor_data);
                            debug!("Received data from {}: temp={:.2}°C, humidity={:.2}%, pressure={:.2} hPa",
                              addr_str, log_data.temperature, log_data.humidity, log_data.pressure/*, log_data.acceleration_x, log_data.acceleration_y, log_data.acceleration_z, log_data.movement_counter*/);
                        }
                    }
                }
                Ok(None) => {
                    debug!("No manufacturer data for {}", addr_str);
                }
                Err(e) => {
                    debug!("Failed to get manufacturer data for {}: {}", addr_str, e);
                }
            }
        }
    }

    Ok(data)
}
