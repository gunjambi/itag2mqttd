
## itag2mqttd

This daemon monitors iTag Bluetooth devices using BlueZ and publishes the data over MQTT. The goal is to expose iTag devices as smart buttons for Home Assistant.

Usage: `./itag2mqttd example_config.ini`

## WARNING

As reported elsewhere too **iTags are very unreliable and WILL BEEP on connection loss.** Do not expect iTags to be fit for any purpose. Even if you were deaf, connection drops and irregular inability to reconnect render their use as a smart button useless. These properties also make it pretty bad keyfinder.

 

## Q&A

**Why not [itag-mqtt-bridge](https://github.com/tomasgatial/itag-mqtt-bridge)?**

Because it doesn't work and it's dependencies have been abandoned.

**Why not [itag-mqtt](https://github.com/onderweg/itag-mqtt)?**

Because Rust is so much better and blazing fast and securererer. Or maybe I didn't notice it before starting this toy project for learning Rust.

**How to suppress the annoying beeping of the iTag device?**

Legends tell of some devices supporting a Link Loss Service that could be configured. My iTags don't. The most reliable way to silence the device is to open it and physically sever the connection to the beeper.

**Why autodiscovery of adapters?**

This is convenient when trying to find a nice position for the adapter. This allows detaching the USB bluetooth adapter from an USB extension cable and putting it back without needing to restart the daemon. That's the theory anyway. In practice, nothing really works well with these things :^)

**Is using multiple adapters at the same time a good idea?**

No.

**Well if it's not a good idea, why is it supported then?**

Nothing is supported. Software is provided as-is with no express or implied warranty. See LICENSE for details.

**Why is the MQTT topic `itag/` and not `homeassistant/` and thus not auto-discovered?**

Because I lost interest after the hardware turned out to be too unreliable for my use-cases.

**Why is this -d if it doesn't even damonize itself?**

Use daemontools :P
(or systemd like all normal people)
