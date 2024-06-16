// Author: Jarkko Pöyry
// License: GPL2

use crate::config::Config;
use rumqttc::{AsyncClient, MqttOptions, QoS};
use std::time::Duration;
use tokio::task;

pub struct MqttClient {
    client: AsyncClient,
}

impl MqttClient {
    pub fn new(config: &Config) -> MqttClient {
        let mut options =
            MqttOptions::new("itag2mqttd", config.mqtt_host.clone(), config.mqtt_port);
        options.set_keep_alive(Duration::from_secs(60));

        let (client, mut eventloop) = AsyncClient::new(options, 10);

        task::spawn(async move {
            loop {
                let _ = eventloop.poll().await;
            }
        });

        return MqttClient { client: client };
    }

    pub fn publish_device(
        &self,
        device_id: &[u8; 6],
        retained: bool,
        is_present: bool,
        is_button_clicked: bool,
    ) {
        let device_id_str = device_id
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        let presence_topic = format!("itag/{}/presence", device_id_str);
        let button_topic = format!("itag/{}/button/click", device_id_str);
        let false_bytes: [u8; 1] = ['0' as u8];
        let true_bytes: [u8; 1] = ['1' as u8];

        // \note: we use try_publish instead of publish. This avoids backpressure
        //        which we do not want in the itag loop. If we would get backpressure,
        //        events are discarded. That is less bad than blocking the itag loop.
        let _ = self.client.try_publish(
            presence_topic,
            QoS::AtLeastOnce,
            retained,
            if is_present { true_bytes } else { false_bytes },
        );
        let _ = self.client.try_publish(
            button_topic,
            QoS::AtLeastOnce,
            retained,
            if is_button_clicked {
                true_bytes
            } else {
                false_bytes
            },
        );
    }
}
