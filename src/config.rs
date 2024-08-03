// Author: Jarkko Pöyry
// See LICENSE for License

use configparser::ini::Ini;
use std::format;

pub struct Config {
    pub mqtt_host: String,
    pub mqtt_port: u16,
    pub bt_adapters: Vec<String>,
}

impl Config {
    pub fn read(path: &str) -> Result<Config, String> {
        let mut config = Ini::new();
        config.load(path)?;

        return match Config::parse_config(config) {
            Ok(config) => Ok(config),
            Err(err) => Err(format!("{0}.\nHint: Format must be:\n[mqtt]\nhost=xxx\nport=xxx\n[bluetooth]\nadapters=hci1,xxx\n", err))
        };
    }

    fn parse_config(config: Ini) -> Result<Config, String> {
        let mqtt_host = config
            .get("mqtt", "host")
            .ok_or("Missing 'host' in [mqtt] block")?;
        let mqtt_port = config
            .getint("mqtt", "port")?
            .ok_or("Missing 'port' in [mqtt] block")?;
        let adapters = config
            .get("bluetooth", "adapters")
            .ok_or("Missing 'adapters' in [bluetooth] block")?;

        let mqtt_port_u16 = match u16::try_from(mqtt_port) {
            Ok(mqtt_port_u16) => mqtt_port_u16,
            Err(_err) => return Err("Invalid mqtt port".to_string()),
        };

        let mut adapters_list: Vec<String> = Vec::new();
        for adapter in adapters.split(",") {
            let adapter_name = adapter.trim().to_string();
            if adapter_name.len() == 0 {
                continue;
            }
            adapters_list.push(adapter_name)
        }

        return Ok(Config {
            mqtt_host: mqtt_host,
            mqtt_port: mqtt_port_u16,
            bt_adapters: adapters_list,
        });
    }

    pub fn is_adapter_allowed(&self, adapter_name: &str) -> bool {
        // If there is no whitelist, then every adapter is accepted
        if self.bt_adapters.len() == 0 {
            return true;
        }
        for adapter in &self.bt_adapters {
            if adapter == adapter_name {
                return true;
            }
        }
        return false;
    }
}
