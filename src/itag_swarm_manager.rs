// Author: Jarkko Pöyry
// License: GPL2

mod device_actor;

use crate::config::Config;
use crate::itag_swarm_manager::device_actor::DeviceActor;
use crate::mqtt_client::MqttClient;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
use tokio_stream::StreamExt;

pub struct ITagSwarmManager {
    actors: Mutex<HashMap<bluer::Address, Arc<DeviceActor>>>,
    config: Config,
    mqttc: Arc<MqttClient>,
}

impl ITagSwarmManager {
    pub fn new(config: Config, mqttc: MqttClient) -> ITagSwarmManager {
        return ITagSwarmManager {
            actors: Mutex::new(HashMap::new()),
            config: config,
            mqttc: Arc::new(mqttc),
        };
    }

    pub async fn run_async(self, session: bluer::Session) {
        let manager = Arc::new(self);

        let adapter_names = match session.adapter_names().await {
            Ok(session) => session,
            Err(err) => {
                panic!("Cannot enumerate bluetooth adapters {0}", err);
            }
        };

        if adapter_names.len() == 0 {
            println!("Warning. No bluetooth adapters found");
        }

        let stream = match session.events().await {
            Ok(stream) => stream,
            Err(err) => {
                panic!("Cannot open bluetooth adapters event stream {0}", err);
            }
        };

        let mut adapters: HashSet<String> = HashSet::new();

        // Apply already existing adapters
        for adapter_name in adapter_names {
            handle_new_adapter(manager.clone(), &session, &mut adapters, adapter_name).await;
        }

        if adapters.len() == 0 {
            println!("Warning. No matching bluetooth adapters present. Adapters may appear later with hot plugging.");
        }

        // Poll for adapter updates
        tokio::pin!(stream);
        while let Some(adapter_event) = stream.next().await {
            match adapter_event {
                bluer::SessionEvent::AdapterAdded(adapter_name) => {
                    handle_new_adapter(manager.clone(), &session, &mut adapters, adapter_name)
                        .await;
                }
                bluer::SessionEvent::AdapterRemoved(adapter_name) => {
                    handle_remove_adapter(&mut adapters, adapter_name).await;
                }
            }
        }
    }
}

async fn handle_new_adapter(
    manager: Arc<ITagSwarmManager>,
    session: &bluer::Session,
    adapters: &mut HashSet<String>,
    adapter_name: String,
) {
    if !manager.config.is_adapter_allowed(&adapter_name) {
        return;
    }

    let adapter = match session.adapter(&adapter_name) {
        Ok(adapter) => adapter,
        Err(err) => {
            println!(
                "Warning! Cannot get bluetooth adapter {0}: {1}",
                adapter_name, err
            );
            return;
        }
    };

    match adapter.set_powered(true).await {
        Ok(()) => {}
        Err(err) => {
            println!(
                "Warning! Cannot set bluetooth adapter {0} powered: {1}",
                adapter_name, err
            );
        }
    };

    let address = match adapter.address().await {
        Ok(address) => address,
        Err(err) => {
            println!("Warning! Cannot get bluetooth adapter address {0}", err);
            return;
        }
    };

    // already inserted?
    if !adapters.insert(adapter_name.clone()) {
        return;
    }

    println!("Found adapter {} ({})", adapter_name, address);
    tokio::spawn(async move { poll_adapter(manager, address, adapter).await });
}

async fn handle_remove_adapter(adapters: &mut HashSet<String>, adapter_name: String) {
    // poll tasks will clear theselves automatically
    adapters.remove(&adapter_name);
}

async fn poll_adapter(
    manager: Arc<ITagSwarmManager>,
    adapter_address: bluer::Address,
    adapter: bluer::Adapter,
) {
    let adapter = Arc::new(adapter);

    // Poll only LE devices
    let filter = bluer::DiscoveryFilter {
        transport: bluer::DiscoveryTransport::Le,
        ..Default::default()
    };

    match adapter.set_discovery_filter(filter).await {
        Ok(()) => {}
        Err(err) => {
            println!("Warning! Couldn't set LE-only discovery filter: {}", err);
        }
    };

    let stream = match adapter.discover_devices_with_changes().await {
        Ok(stream) => stream,
        Err(err) => {
            println!(
                "Warning! Cannot get bluetooth adapter event stream: {}",
                err
            );
            return;
        }
    };

    tokio::pin!(stream);
    while let Some(event) = stream.next().await {
        match event {
            bluer::AdapterEvent::DeviceAdded(device_address) => {
                handle_device_updated(&manager, adapter_address, adapter.clone(), device_address)
                    .await;
            }
            bluer::AdapterEvent::DeviceRemoved(device_address) => {
                handle_device_removed(&manager, adapter_address, device_address).await;
            }
            bluer::AdapterEvent::PropertyChanged(_property_change) => {}
        }
    }
}

async fn handle_device_updated(
    manager: &ITagSwarmManager,
    adapter_address: bluer::Address,
    adapter: Arc<bluer::Adapter>,
    device_address: bluer::Address,
) -> () {
    let device = match adapter.device(device_address) {
        Ok(device) => device,
        Err(_) => {
            // Device is gone, handle as gone
            handle_device_removed(manager, adapter_address, device_address).await;
            return;
        }
    };

    on_device_discovered(manager, adapter_address, adapter, device_address, device).await
}

async fn handle_device_removed(
    manager: &ITagSwarmManager,
    adapter_address: bluer::Address,
    device_address: bluer::Address,
) {
    on_device_lost(manager, adapter_address, device_address).await
}

async fn on_device_discovered(
    manager: &ITagSwarmManager,
    adapter_address: bluer::Address,
    adapter: Arc<bluer::Adapter>,
    device_address: bluer::Address,
    device: bluer::Device,
) {
    // Check if the device is iTAG
    match device.name().await {
        Ok(Some(name)) => {
            if name.to_ascii_uppercase().trim() != "ITAG" {
                // Not iTag, ignore
                return;
            }
        }
        Ok(None) => {
            // Not iTag, ignore
            return;
        }
        Err(_error) => {
            // Device probably got lost. It might have been an iTag, so clear the state
            on_device_lost(manager, adapter_address, device_address).await;
            return;
        }
    };

    // Check if the device is in range
    match device.rssi().await {
        Ok(Some(_rssi)) => {}
        Ok(None) => {
            // Device not present
            on_device_lost(manager, adapter_address, device_address).await;
            return;
        }
        Err(_error) => {
            // Device got lost.
            on_device_lost(manager, adapter_address, device_address).await;
            return;
        }
    };

    // Find device monitor and inform it
    let mut actors = manager.actors.lock().unwrap();
    let actor = match actors.get(&device_address) {
        Some(actor) => actor.clone(),
        None => {
            let actor = DeviceActor::new(&device_address, manager.mqttc.clone());
            _ = actors.insert(device_address, actor.clone());
            actor
        }
    };
    actor.device_discovered(adapter_address, adapter, device);
}

async fn on_device_lost(
    manager: &ITagSwarmManager,
    adapter_address: bluer::Address,
    device_address: bluer::Address,
) {
    // Find device monitor and inform it
    let actors = manager.actors.lock().unwrap();
    match actors.get(&device_address) {
        Some(actor) => actor.device_removed(adapter_address),
        None => {}
    }
}
