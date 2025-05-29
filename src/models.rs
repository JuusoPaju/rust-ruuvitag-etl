use time::OffsetDateTime;

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
