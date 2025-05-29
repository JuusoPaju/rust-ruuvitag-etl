use std::collections::HashMap;
use std::env;

#[derive(Debug, Clone)]
pub struct SensorConfig {
    pub tags: HashMap<String, String>,
    pub database_url: String,
}

impl SensorConfig {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Load environment variables
        dotenv::dotenv().ok();

        let database_url =
            env::var("DATABASE_URL").map_err(|_| "DATABASE_URL environment variable not set")?;

        let mut tags = HashMap::new();

        // Try RUUVI_TAGS format first
        if let Ok(ruuvi_tags) = env::var("RUUVI_TAGS") {
            println!("Found RUUVI_TAGS: '{}'", ruuvi_tags);
            for pair in ruuvi_tags.split(',') {
                let pair = pair.trim();
                println!("Processing pair: '{}'", pair);
                if !pair.is_empty() {
                    if let Some((mac, name)) = pair.split_once('=') {
                        let mac = mac.trim();
                        let name = name.trim();
                        println!("Found MAC: '{}', Name: '{}'", mac, name);
                        if !mac.is_empty() && !name.is_empty() {
                            tags.insert(mac.to_string(), name.to_string());
                        }
                    } else {
                        println!("Failed to split pair: '{}'", pair);
                    }
                }
            }
        } else {
            // Fallback to individual environment variables
            println!("RUUVI_TAGS environment variable not found, trying individual variables");
            for (key, value) in env::vars() {
                if key.starts_with("RUUVI_TAG_") && key.ends_with("_MAC") {
                    if let Some(index) = key
                        .strip_prefix("RUUVI_TAG_")
                        .and_then(|s| s.strip_suffix("_MAC"))
                    {
                        let name_key = format!("RUUVI_TAG_{}_NAME", index);
                        if let Ok(name) = env::var(&name_key) {
                            tags.insert(value, name);
                        }
                    }
                }
            }
        }

        println!("Total tags loaded: {}", tags.len());
        for (mac, name) in &tags {
            println!("Tag: {} -> {}", mac, name);
        }

        if tags.is_empty() {
            return Err("No RuuviTag sensors configured. Please set RUUVI_TAGS or RUUVI_TAG_<N>_MAC/RUUVI_TAG_<N>_NAME environment variables".into());
        }

        Ok(SensorConfig { tags, database_url })
    }
}
