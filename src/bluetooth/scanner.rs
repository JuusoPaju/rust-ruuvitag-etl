use futures_util::StreamExt;
use log::{debug, error, warn};
use std::collections::HashMap;
use tokio::time::{sleep, Duration};

use crate::config::SensorConfig;
use crate::models::RuuviData;

const RUUVITAG_MANUFACTURER_ID: u16 = 0x0499;
const DATA_FORMAT: u8 = 5;
const SCAN_DURATION_SECS: u64 = 20;

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

    match (|| -> Result<RuuviData, Box<dyn std::error::Error>> {
        let temperature = i16::from_be_bytes([data[1], data[2]]) as f32 * 0.005;
        let humidity = (u16::from_be_bytes([data[3], data[4]]) as f32 * 0.0025).min(100.0);
        let pressure = (u16::from_be_bytes([data[5], data[6]]) as f32 + 50000.0) / 100.0;

        let acc_x = i16::from_be_bytes([data[7], data[8]]) as f32 * 0.001;
        let acc_y = i16::from_be_bytes([data[9], data[10]]) as f32 * 0.001;
        let acc_z = i16::from_be_bytes([data[11], data[12]]) as f32 * 0.001;

        let movement_counter = data[15];

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

pub async fn scan_for_ruuvitags(
    config: &SensorConfig,
) -> Result<HashMap<String, RuuviData>, Box<dyn std::error::Error>> {
    let mut data = HashMap::new();

    let session = match bluer::Session::new().await {
        Ok(session) => session,
        Err(e) => {
            error!("Failed to create Bluetooth session: {}", e);
            return Err(e.into());
        }
    };

    let adapter = match session.default_adapter().await {
        Ok(adapter) => adapter,
        Err(e) => {
            error!("Failed to get default Bluetooth adapter: {}", e);
            return Err(e.into());
        }
    };

    if let Err(e) = adapter.set_powered(true).await {
        error!("Failed to power on adapter: {}", e);
        return Err(e.into());
    }

    let filter = bluer::DiscoveryFilter {
        transport: bluer::DiscoveryTransport::Le,
        duplicate_data: false,
        ..Default::default()
    };

    if let Err(e) = adapter.set_discovery_filter(filter).await {
        warn!("Failed to set discovery filter: {}", e);
    }

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

    sleep(Duration::from_secs(SCAN_DURATION_SECS)).await;
    discovery_handle.abort();

    let devices = match adapter.device_addresses().await {
        Ok(devices) => devices,
        Err(e) => {
            error!("Failed to get device addresses: {}", e);
            return Err(e.into());
        }
    };

    for addr in devices {
        let device = match adapter.device(addr) {
            Ok(device) => device,
            Err(_) => continue,
        };

        let addr_str = device.address().to_string().to_uppercase();

        if config.tags.contains_key(&addr_str) {
            match device.manufacturer_data().await {
                Ok(Some(manufacturer_data)) => {
                    if let Some(ruuvi_data) = manufacturer_data.get(&RUUVITAG_MANUFACTURER_ID) {
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
