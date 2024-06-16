// Author: Jarkko Pöyry
// License: GPL2

use crate::MqttClient;
use bluer::Uuid;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use tokio_stream::Stream;
use tokio_stream::StreamExt;

pub struct DeviceActor {
    device_address: bluer::Address,
    sender: mpsc::UnboundedSender<DeviceMessage>,
    mqttc: Arc<MqttClient>,
}

enum DeviceMessage {
    Stabilized,
    DeviceDiscovered {
        adapter_address: bluer::Address,
        device: bluer::Device,
    },
    DeviceLost {
        adapter_address: bluer::Address,
    },
    ButtonMonitorExit,
}

impl DeviceActor {
    pub fn new(device_address: &bluer::Address, mqttc: Arc<MqttClient>) -> Arc<DeviceActor> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let device = Arc::new(DeviceActor {
            device_address: device_address.clone(),
            sender: sender,
            mqttc: mqttc,
        });

        // Create device monitor for it
        let device_copy = device.clone();
        tokio::spawn(async move { device_manager_loop(device_copy, receiver).await });

        return device;
    }

    pub fn device_discovered(
        &self,
        adapter_address: bluer::Address,
        _adapter: Arc<bluer::Adapter>,
        device: bluer::Device,
    ) {
        self.send(DeviceMessage::DeviceDiscovered {
            adapter_address,
            device,
        });
    }

    pub fn device_removed(&self, adapter_address: bluer::Address) {
        self.send(DeviceMessage::DeviceLost { adapter_address });
    }

    fn send(&self, message: DeviceMessage) {
        self.sender.send(message).unwrap();
    }
}

struct ConnectedAdapter {
    device: Arc<bluer::Device>,
}

async fn device_manager_loop(
    actor: Arc<DeviceActor>,
    mut receiver: mpsc::UnboundedReceiver<DeviceMessage>,
) {
    let mut discovered_on_adapter: HashMap<bluer::Address, ConnectedAdapter> = HashMap::new();
    let mut stabilized: bool = false;
    let mut has_button_monitor: bool = false;

    // When device is seen, publish it on MQTT as retained but without it being present
    actor
        .mqttc
        .publish_device(&actor.device_address, true, false, false);

    // Upon first discovery, we wait a second to make sure all adapters have stabilized
    {
        let actor = actor.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(1000)).await;
            actor.send(DeviceMessage::Stabilized {})
        });
    }

    while let Some(event) = receiver.recv().await {
        match event {
            DeviceMessage::Stabilized {} => {
                stabilized = true;
            }
            DeviceMessage::DeviceDiscovered {
                adapter_address,
                device,
            } => {
                let discovered = ConnectedAdapter {
                    device: Arc::new(device),
                };
                if let None = discovered_on_adapter.insert(adapter_address, discovered) {
                    println!(
                        "Discovered {0} on {1}",
                        actor.device_address, adapter_address
                    );
                }
            }
            DeviceMessage::DeviceLost { adapter_address } => {
                if let Some(_) = discovered_on_adapter.remove(&adapter_address) {
                    if discovered_on_adapter.is_empty() {
                        println!(
                            "Device {0} no longer visible on any adapter",
                            actor.device_address
                        );
                    }
                }
            }
            DeviceMessage::ButtonMonitorExit {} => {
                has_button_monitor = false;
            }
        }

        if !stabilized {
            continue;
        }

        // Connect to the device on the best adapter
        if !has_button_monitor {
            if let Some(adapter) = get_best_adapter(&discovered_on_adapter).await {
                has_button_monitor = true;

                let actor = actor.clone();
                let device = adapter.device.clone();
                let mqttc = actor.mqttc.clone();
                tokio::spawn(async move {
                    _ = monitor_itag_button(&device, mqttc).await;
                    actor.send(DeviceMessage::ButtonMonitorExit {})
                });
            }
        }
    }
}

async fn get_best_adapter(
    adapters: &HashMap<bluer::Address, ConnectedAdapter>,
) -> Option<&ConnectedAdapter> {
    let mut best_candidate: Option<(i16, &ConnectedAdapter)> = None;
    for (_, adapter) in adapters.iter() {
        if let Ok(Some(this_rssi)) = adapter.device.rssi().await {
            if let Some((best_rssi, _)) = best_candidate {
                if this_rssi > best_rssi {
                    best_candidate = Some((this_rssi, adapter));
                }
            } else {
                best_candidate = Some((this_rssi, adapter));
            }
        }
    }

    match best_candidate {
        Some((_, adapter)) => Some(adapter),
        None => None,
    }
}

async fn monitor_itag_button(device: &bluer::Device, mqttc: Arc<MqttClient>) -> Result<(), bluer::Error> {
    // Connect. Connections more than 5 seconds are unlikely to succeed so abort.
    if !device.is_connected().await? {
        let timeout = sleep(Duration::from_secs(5));
        tokio::pin!(timeout);
        tokio::select! {
            connect_result = device.connect() => {
                match connect_result {
                    Ok(()) => {},
                    Err(error) =>
                    {
                        device.disconnect().await?;
                        return Err(error)
                    }
                }
            },
            _ = &mut timeout => {
                device.disconnect().await?;
                return Err(bluer::Error { kind: bluer::ErrorKind::DoesNotExist, message: String::from("connection timeout") })
            }
        }
    }

    let events = device.events().await?;
    let button_notify = get_button_notify_stream(device).await?;

    // On connect, the itag beeps. Send manual alert to override the auto-alert.
    _ = stop_beeping(device).await;

    // Mark as present
    // \note: not retained
    mqttc.publish_device(&device.address(), false, true, false);

    tokio::pin!(events);
    tokio::pin!(button_notify);
    loop {
        tokio::select! {
            event_maybe = events.next() => {
                match event_maybe {
                    Some(_event) => {
                        // println!("On received event {:?}", event);
                    },
                    None => {
                        break;
                    }
                }
            },
            event_maybe = button_notify.next() => {
                match event_maybe {
                    Some(event) => {
                        // Received button. Flip button.
                        println!("On received button {:?}", event);
                        mqttc.publish_device(&device.address(), false, true, true);
                        mqttc.publish_device(&device.address(), false, true, false);
                    },
                    None => {
                        break;
                    }
                }
            },
        }
    }

    // Mark as absent
    // \note: not retained
    mqttc.publish_device(&device.address(), false, false, false);

    Ok(())
}

static BUTTON_SERVICE: Uuid = Uuid::from_u128(0x0000ffe0_0000_1000_8000_00805f9b34fb);
static BUTTON_CHARACTERISTIC: Uuid = Uuid::from_u128(0x0000ffe1_0000_1000_8000_00805f9b34fb);

async fn get_button_notify_stream(
    device: &bluer::Device,
) -> Result<impl Stream<Item = Vec<u8>>, bluer::Error> {
    for service in device.services().await? {
        let uuid = service.uuid().await?;
        if uuid != BUTTON_SERVICE {
            continue;
        }

        for char in service.characteristics().await? {
            let uuid = char.uuid().await?;
            if uuid != BUTTON_CHARACTERISTIC {
                continue;
            }

            return char.notify().await;
        }
    }
    Err(bluer::Error {
        kind: bluer::ErrorKind::DoesNotExist,
        message: String::from("No button stream"),
    })
}

static IMMEDIATE_ALERT_SERVICE: Uuid = Uuid::from_u128(0x00001802_0000_1000_8000_00805f9b34fb);
static ALERT_LEVEL_CHARACTERISTIC: Uuid = Uuid::from_u128(0x00002a06_0000_1000_8000_00805f9b34fb);

async fn stop_beeping(device: &bluer::Device) -> Result<(), bluer::Error> {
    for service in device.services().await? {
        let uuid = service.uuid().await?;
        if uuid != IMMEDIATE_ALERT_SERVICE {
            continue;
        }

        for char in service.characteristics().await? {
            let uuid = char.uuid().await?;
            if uuid != ALERT_LEVEL_CHARACTERISTIC {
                continue;
            }

            let data = vec![0x0];
            char.write(&data).await?
        }
    }
    Ok(())
}
