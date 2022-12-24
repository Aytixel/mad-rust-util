use std::time::Duration;

use tokio::time::Instant;

const MAX_BUFFER_SIZE: usize = 32768;
const MAX_PACKET_SIZE: usize = 32760;
const PING_PACKET_DELAY: Duration = Duration::from_millis(500);
const PACKET_TIMEOUT_DELAY: Duration = Duration::from_secs(1);

#[derive(Clone)]
pub enum ConnectionState {
    Start,
    Data(Vec<u8>),
    End,
}

#[derive(Clone)]
enum Life {
    Ping,
    Timeout,
}

impl Life {
    fn check_life(last: Instant) -> Life {
        match last.elapsed() {
            x if x > PACKET_TIMEOUT_DELAY => Life::Timeout,
            _ => Life::Ping,
        }
    }
}

struct Packet {
    size_option: Option<u64>,
    data: Vec<u8>,
}

impl Packet {
    fn new() -> Self {
        Self {
            size_option: None,
            data: vec![],
        }
    }

    fn recv(&mut self, size: usize, buffer: [u8; MAX_BUFFER_SIZE]) -> Option<Vec<u8>> {
        if size > 0 {
            if let Some(packet_size) = self.size_option {
                self.data.append(&mut buffer[..size].to_vec());

                if self.data.len() as u64 == packet_size {
                    let result_packet = Some(self.data.clone());

                    self.clear();

                    return result_packet;
                }
            } else if size >= 8 {
                if let Ok(packet_size_bytes) = buffer[..8].try_into() {
                    self.size_option = Some(u64::from_be_bytes(packet_size_bytes));
                }
            }
        }

        None
    }

    fn clear(&mut self) {
        self.size_option = None;
        self.data.clear();
    }
}

pub use server::Server;

pub mod server {
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::Duration;

    use super::{ConnectionState, Life, Packet, MAX_BUFFER_SIZE, PING_PACKET_DELAY};

    use crate::connection::MAX_PACKET_SIZE;
    use crate::thread::DualChannel;

    use hashbrown::HashMap;
    use tokio::net::UdpSocket;
    use tokio::spawn;
    use tokio::sync::Mutex;
    use tokio::time::{interval, sleep, Instant};

    struct Client {
        last_recv: Instant,
        socket_addr: SocketAddr,
        child: DualChannel<(SocketAddr, ConnectionState)>,
        packet: Packet,
    }

    impl Client {
        async fn new(
            socket_addr: SocketAddr,
            child: DualChannel<(SocketAddr, ConnectionState)>,
        ) -> Self {
            child
                .send_async((socket_addr, ConnectionState::Start))
                .await
                .ok();

            Self {
                last_recv: Instant::now(),
                socket_addr,
                child,
                packet: Packet::new(),
            }
        }

        async fn recv(&mut self, size: usize, buffer: [u8; 32_768]) {
            self.last_recv = Instant::now();

            if let Some(data) = self.packet.recv(size, buffer) {
                self.child
                    .send_async((self.socket_addr, ConnectionState::Data(data)))
                    .await
                    .ok();
            }
        }

        fn check_life(&self) -> Life {
            Life::check_life(self.last_recv)
        }
    }

    impl PartialEq for Client {
        fn eq(&self, other: &Self) -> bool {
            self.socket_addr == other.socket_addr
        }
    }

    pub struct Server {
        pub dual_channel: DualChannel<(SocketAddr, ConnectionState)>,
    }

    impl Server {
        pub async fn new() -> Self {
            let (host, child) = DualChannel::<(SocketAddr, ConnectionState)>::new();

            if let Ok(socket) = UdpSocket::bind("127.0.0.1:651").await {
                let socket = Arc::new(socket);
                let client_hashmap_mutex = Arc::new(Mutex::new(HashMap::new()));

                // connection timeout
                {
                    let child = child.clone();
                    let socket = socket.clone();
                    let client_hashmap_mutex = client_hashmap_mutex.clone();

                    spawn(async move {
                        let mut interval_ = interval(PING_PACKET_DELAY);

                        loop {
                            for socket_addr in async {
                                client_hashmap_mutex
                                    .lock()
                                    .await
                                    .keys()
                                    .cloned()
                                    .collect::<Vec<SocketAddr>>()
                            }
                            .await
                            {
                                match async {
                                    let client_hashmap = client_hashmap_mutex.lock().await;
                                    let client: &Client = client_hashmap.get(&socket_addr).unwrap();

                                    client.check_life().clone()
                                }
                                .await
                                {
                                    Life::Ping => {
                                        socket.send_to(&vec![], socket_addr).await.ok();
                                    }
                                    Life::Timeout => {
                                        client_hashmap_mutex.lock().await.remove(&socket_addr);
                                        child
                                            .send_async((socket_addr, ConnectionState::End))
                                            .await
                                            .ok();
                                    }
                                }
                            }

                            interval_.tick().await;
                        }
                    })
                };

                // from client to server
                {
                    let child = child.clone();
                    let socket = socket.clone();
                    let client_hashmap_mutex = client_hashmap_mutex.clone();

                    spawn(async move {
                        let mut buffer = [0; MAX_BUFFER_SIZE];

                        loop {
                            if let Ok((size, socket_addr)) = socket.recv_from(&mut buffer).await {
                                let mut client_hashmap = client_hashmap_mutex.lock().await;

                                // connection end
                                if size == MAX_BUFFER_SIZE {
                                    child
                                        .send_async((socket_addr, ConnectionState::End))
                                        .await
                                        .ok();
                                    client_hashmap.remove(&socket_addr);
                                    continue;
                                }

                                socket.send_to(&vec![], socket_addr).await.ok();

                                match client_hashmap.get_mut(&socket_addr) {
                                    Some(client) => client,
                                    None => {
                                        client_hashmap.insert(
                                            socket_addr,
                                            Client::new(socket_addr, child.clone()).await,
                                        );
                                        client_hashmap.get_mut(&socket_addr).unwrap()
                                    }
                                }
                                .recv(size, buffer)
                                .await;
                            }

                            buffer = [0; MAX_BUFFER_SIZE];
                        }
                    })
                };

                // from server to client
                spawn(async move {
                    loop {
                        if let Ok((socket_addr, ConnectionState::Data(buffer))) =
                            child.recv_async().await
                        {
                            if client_hashmap_mutex.lock().await.contains_key(&socket_addr) {
                                socket
                                    .send_to(&buffer.len().to_be_bytes(), socket_addr)
                                    .await
                                    .ok();

                                for chunk in buffer.chunks(MAX_PACKET_SIZE) {
                                    socket.send_to(chunk, socket_addr).await.ok();

                                    sleep(Duration::from_nanos(10)).await;
                                }
                            }
                        }
                    }
                });
            }

            Self { dual_channel: host }
        }
    }
}

pub use client::Client;

pub mod client {
    use std::sync::Arc;
    use std::time::Duration;

    use super::{ConnectionState, Life, Packet, MAX_BUFFER_SIZE, PING_PACKET_DELAY};

    use crate::connection::MAX_PACKET_SIZE;
    use crate::thread::DualChannel;
    use crate::time::TIMEOUT_1S;

    use tokio::net::UdpSocket;
    use tokio::spawn;
    use tokio::sync::{Notify, Mutex};
    use tokio::time::{interval, sleep, Instant, MissedTickBehavior};

    pub struct Client {
        pub dual_channel: DualChannel<ConnectionState>,
    }

    impl Client {
        pub async fn new() -> Self {
            let (host, child) = DualChannel::<ConnectionState>::new();

            if let Ok(socket) = UdpSocket::bind("127.0.0.1:0").await {
                socket.connect("127.0.0.1:651").await.ok();

                let socket = Arc::new(socket);

                spawn(async move {
                    let mut interval_ = interval(TIMEOUT_1S);

                    interval_.set_missed_tick_behavior(MissedTickBehavior::Skip);

                    loop {
                        let child = child.clone();
                        let socket = socket.clone();

                        if UdpSocket::bind("127.0.0.1:651").await.is_err() {
                            let shutdown_notifier = Arc::new(Notify::new());
                            let last_recv_mutex = Arc::new(Mutex::new(Instant::now()));
                            let mut packet = Packet::new();

                            // connection timeout
                            let connection_timeout_handle = {
                                let socket = socket.clone();
                                let last_recv_mutex = last_recv_mutex.clone();
                                let shutdown_notifier = shutdown_notifier.clone();

                                spawn(async move {
                                    let mut interval_ = interval(PING_PACKET_DELAY);

                                    loop {
                                        {
                                            match Life::check_life(*last_recv_mutex.lock().await) {
                                                Life::Ping => {
                                                    socket.send(&vec![]).await.ok();
                                                }
                                                Life::Timeout => {
                                                    shutdown_notifier.notify_one();
                                                }
                                            }
                                        }

                                        interval_.tick().await;
                                    }
                                })
                            };

                            // from server to client
                            let from_server_to_client = {
                                let child = child.clone();
                                let socket = socket.clone();
                                let last_recv_mutex = last_recv_mutex.clone();

                                spawn(async move {
                                    let mut buffer = [0; MAX_BUFFER_SIZE];

                                    loop {
                                        if let Ok(size) = socket.recv(&mut buffer).await {
                                            // connection end
                                            if size == MAX_BUFFER_SIZE {
                                                child.send_async(ConnectionState::End).await.ok();
                                                break;
                                            }

                                            *last_recv_mutex.lock().await = Instant::now();

                                            if let Some(data) = packet.recv(size, buffer) {
                                                child
                                                    .send_async(ConnectionState::Data(data))
                                                    .await
                                                    .ok();
                                            }
                                        }

                                        buffer = [0; MAX_BUFFER_SIZE];
                                    }
                                })
                            };

                            // from client to server
                            let from_client_to_server = {
                                let child = child.clone();

                                spawn(async move {
                                    loop {
                                        if let Ok(ConnectionState::Data(buffer)) =
                                            child.recv_async().await
                                        {
                                            socket.send(&buffer.len().to_be_bytes()).await.ok();

                                            for chunk in buffer.chunks(MAX_PACKET_SIZE) {
                                                socket.send(chunk).await.ok();

                                                sleep(Duration::from_nanos(10)).await;
                                            }
                                        }
                                    }
                                })
                            };

                            sleep(Duration::from_millis(100)).await;

                            child.send_async(ConnectionState::Start).await.ok();

                            // shutdown all task
                            shutdown_notifier.notified().await;
                            connection_timeout_handle.abort();
                            from_server_to_client.abort();
                            from_client_to_server.abort();
                            child.send_async(ConnectionState::End).await.ok();
                        }

                        interval_.tick().await;
                    }
                });
            }

            Self { dual_channel: host }
        }
    }
}

pub use command::CommandTrait;

pub mod command {
    use serde::{Deserialize, Serialize};

    pub trait CommandTrait {
        fn to_bytes(&mut self) -> Vec<u8>;

        fn from_bytes(data: Vec<u8>) -> Self;
    }

    const DRIVER_CONFIGURATION_DESCRIPTOR_ID: u8 = 0;
    const DEVICE_LIST_ID: u8 = 1;
    const REQUEST_DEVICE_CONFIG_ID: u8 = 2;
    const DEVICE_CONFIG_ID: u8 = 3;
    const UNKNOWN_ID: u8 = 255;

    #[derive(Debug, Clone)]
    pub enum Commands {
        DriverConfigurationDescriptor(DriverConfigurationDescriptor),
        DeviceList(DeviceList),
        RequestDeviceConfig(RequestDeviceConfig),
        DeviceConfig(DeviceConfig),
        Unknown,
    }

    impl Commands {
        pub fn test(self, value: &Vec<u8>) -> bool {
            self.into() == value[0]
        }
    }

    impl Commands {
        fn into(self) -> u8 {
            match self {
                Self::DriverConfigurationDescriptor(_) => DRIVER_CONFIGURATION_DESCRIPTOR_ID,
                Self::DeviceList(_) => DEVICE_LIST_ID,
                Self::RequestDeviceConfig(_) => REQUEST_DEVICE_CONFIG_ID,
                Self::DeviceConfig(_) => DEVICE_CONFIG_ID,
                Self::Unknown => UNKNOWN_ID,
            }
        }
    }

    impl From<Vec<u8>> for Commands {
        fn from(value: Vec<u8>) -> Self {
            match value[0] {
                DRIVER_CONFIGURATION_DESCRIPTOR_ID => Self::DriverConfigurationDescriptor(
                    DriverConfigurationDescriptor::from_bytes(value),
                ),
                DEVICE_LIST_ID => Self::DeviceList(DeviceList::from_bytes(value)),
                REQUEST_DEVICE_CONFIG_ID => {
                    Self::RequestDeviceConfig(RequestDeviceConfig::from_bytes(value))
                }
                DEVICE_CONFIG_ID => Self::DeviceConfig(DeviceConfig::from_bytes(value)),
                _ => Self::Unknown,
            }
        }
    }

    impl PartialEq<u8> for Commands {
        fn eq(&self, other: &u8) -> bool {
            let value: u8 = self.clone().into();

            value == *other
        }
    }

    impl PartialEq<Vec<u8>> for Commands {
        fn eq(&self, other: &Vec<u8>) -> bool {
            *self == other[0]
        }
    }

    #[derive(Serialize, Deserialize, Clone, Default, Debug)]
    pub struct DriverConfigurationDescriptor {
        pub id: u8,
        pub vid: u16,
        pub pid: u16,
        pub device_name: String,
        pub device_icon: Vec<u8>,
        pub mode_count: u8,
        pub shift_mode_count: u8,
        pub button_name_vec: Vec<String>,
    }

    impl DriverConfigurationDescriptor {
        pub fn new(
            vid: u16,
            pid: u16,
            device_name: String,
            device_icon: Vec<u8>,
            mode_count: u8,
            shift_mode_count: u8,
            button_name_vec: Vec<String>,
        ) -> Self {
            Self {
                id: DRIVER_CONFIGURATION_DESCRIPTOR_ID,
                vid,
                pid,
                device_name,
                device_icon,
                mode_count,
                shift_mode_count,
                button_name_vec,
            }
        }
    }

    impl CommandTrait for DriverConfigurationDescriptor {
        fn to_bytes(&mut self) -> Vec<u8> {
            bincode::serialize(&self).unwrap()
        }

        fn from_bytes(data: Vec<u8>) -> Self {
            bincode::deserialize(&data).unwrap()
        }
    }

    #[derive(Serialize, Deserialize, Clone, Default, Debug)]
    pub struct DeviceList {
        pub id: u8,
        pub serial_number_vec: Vec<String>,
    }

    impl DeviceList {
        pub fn new(serial_number_vec: Vec<String>) -> Self {
            Self {
                id: DEVICE_LIST_ID,
                serial_number_vec,
            }
        }
    }

    impl CommandTrait for DeviceList {
        fn to_bytes(&mut self) -> Vec<u8> {
            bincode::serialize(&self).unwrap()
        }

        fn from_bytes(data: Vec<u8>) -> Self {
            bincode::deserialize(&data).unwrap()
        }
    }

    #[derive(Serialize, Deserialize, Clone, Default, Debug)]
    pub struct RequestDeviceConfig {
        pub id: u8,
        pub serial_number: String,
    }

    impl RequestDeviceConfig {
        pub fn new(serial_number: String) -> Self {
            Self {
                id: REQUEST_DEVICE_CONFIG_ID,
                serial_number,
            }
        }
    }

    impl CommandTrait for RequestDeviceConfig {
        fn to_bytes(&mut self) -> Vec<u8> {
            bincode::serialize(&self).unwrap()
        }

        fn from_bytes(data: Vec<u8>) -> Self {
            bincode::deserialize(&data).unwrap()
        }
    }

    #[derive(Serialize, Deserialize, Clone, Default, Debug)]
    pub struct DeviceConfig {
        pub id: u8,
        pub serial_number: String,
        pub config: Vec<[Vec<String>; 2]>,
    }

    impl DeviceConfig {
        pub fn new(serial_number: String, config: Vec<[Vec<String>; 2]>) -> Self {
            Self {
                id: DEVICE_CONFIG_ID,
                serial_number,
                config,
            }
        }
    }

    impl CommandTrait for DeviceConfig {
        fn to_bytes(&mut self) -> Vec<u8> {
            bincode::serialize(&self).unwrap()
        }

        fn from_bytes(data: Vec<u8>) -> Self {
            bincode::deserialize(&data).unwrap()
        }
    }
}
