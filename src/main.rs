// Author: Jarkko Pöyry
// See LICENSE for License

mod config;
mod itag_swarm_manager;
mod mqtt_client;

use crate::config::Config;
use crate::itag_swarm_manager::ITagSwarmManager;
use crate::mqtt_client::MqttClient;
use std::process;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let config_file = match std::env::args().nth(1) {
        Some(path) => path,
        None => {
            eprintln!("Error: Config file path was not given on command line. Usage: itag2mqttd path/to/config.ini");
            process::exit(1);
        }
    };

    let config = match Config::read(&config_file) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Error: Failed to parse config: {0}", err);
            process::exit(1);
        }
    };

    let session = match bluer::Session::new().await {
        Ok(session) => session,
        Err(err) => {
            eprintln!("Cannot open bluetooth session {0}", err);
            process::exit(1);
        }
    };

    let mqttc = MqttClient::new(&config);

    let manager = ITagSwarmManager::new(config, mqttc);
    manager.run_async(session).await;
}
