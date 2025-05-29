/// Data structures for sensor readings and processed data
use time::OffsetDateTime;

/// Raw sensor data decoded from RuuviTag Bluetooth advertisements
///
/// This represents a single reading from a RuuviTag sensor using data format 5.
/// All values are decoded from the 24-byte manufacturer data payload.
#[derive(Debug, Clone)]
pub struct RuuviData {
    pub temperature: f32,
    pub humidity: f32,
    pub pressure: f32,
    pub acceleration_x: f32,
    pub acceleration_y: f32,
    pub acceleration_z: f32,
    pub movement_counter: u8,
}

/// Processed sensor data representing averages over a collection interval
///
/// This structure contains averaged values from multiple RuuviData readings
/// along with metadata about the collection period.
#[derive(Debug, Clone)]
pub struct AverageData {
    pub temperature: f32,
    pub humidity: f32,
    pub pressure: f32,
    pub acceleration_x: f32,
    pub acceleration_y: f32,
    pub acceleration_z: f32,
    pub movement_counter: u32,
    pub time: OffsetDateTime,
    pub name: String,
    pub samples: i32,
}
