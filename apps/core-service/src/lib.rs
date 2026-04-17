use anyhow::Result;
use core_protocol::{negotiate_protocol, ProtocolFrame, ProtocolMessage, VersionNegotiation};
use core_session::{
    bind_discovery_socket, build_server_tls_config, manual_endpoint, session_descriptor,
    DISCOVERY_PORT, SESSION_PORT,
};
use core_topology::load_or_create_topology;
use device_trust::{default_display_name, load_or_create_certificate, load_or_create_identity};
use foundation::{
    append_log, export_diagnostic_snapshot, init_tracing, load_discovery_peers,
    load_or_create_config, load_pending_pairing_requests, save_discovery_peers,
    save_pending_pairing_requests, AppPaths, DiscoveryPeer, PendingPairingRequest,
};
use local_ipc::bind_listener;
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::watch;
use tracing::info;

const DISCOVERY_PEER_TTL_MS: u128 = 8_000;

pub async fn run_core_service() -> Result<()> {
    let paths = AppPaths::from_runtime_env()?;
    let config = load_or_create_config(&paths)?;
    init_tracing(&config.log_level)?;
    append_log(&paths, "core-service startup")?;
    let identity = load_or_create_identity(&paths, &default_display_name())?;
    let certificate = load_or_create_certificate(&paths, &identity)?;
    let endpoint = manual_endpoint(detect_local_host_ip(), SESSION_PORT);
    let session_device = session_descriptor(&identity, &certificate, &endpoint);
    let _session_tls = build_server_tls_config(&paths)?;
    let topology = load_or_create_topology(&paths, identity.device_id, &identity.display_name)?;
    let diagnostic = export_diagnostic_snapshot(&paths, &config)?;

    info!("core-service skeleton started");
    info!(
        device_id = %identity.device_id,
        fingerprint = %certificate.fingerprint_sha256,
        "device identity ready"
    );
    info!(
        "core protocol version: {}",
        core_protocol::CURRENT_PROTOCOL_VERSION
    );
    info!(
        discovery_port = DISCOVERY_PORT,
        session_port = SESSION_PORT,
        address = %session_device.address,
        "session foundation ready"
    );
    info!(
        topology_version = topology.version,
        topology_devices = topology.devices.len(),
        "topology layout ready"
    );
    info!("diagnostic snapshot exported to {}", diagnostic.display());

    let listener = bind_listener().await?;
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let discovery_socket = prepare_discovery_socket()?;
    let discovery_shutdown_rx = shutdown_tx.subscribe();
    let discovery_paths = paths.clone();
    let discovery_device = session_device.clone();
    tokio::spawn(async move {
        if let Err(error) = run_discovery_loop(
            discovery_socket,
            discovery_device,
            discovery_paths,
            discovery_shutdown_rx,
        )
        .await
        {
            tracing::error!(?error, "discovery loop failed");
        }
    });

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_ok() && *shutdown_rx.borrow() {
                    info!("core-service received shutdown signal");
                    append_log(&paths, "core-service shutdown requested")?;
                    break;
                }
            }
            accept_result = listener.accept() => {
                let (stream, _) = accept_result?;
                let shutdown_tx = shutdown_tx.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle_client(stream, shutdown_tx).await {
                        tracing::error!(?error, "failed to handle local-ipc client");
                    }
                });
            }
        }
    }

    info!("core-service skeleton finished");
    append_log(&paths, "core-service shutdown complete")?;

    Ok(())
}

fn prepare_discovery_socket() -> Result<UdpSocket> {
    let socket = bind_discovery_socket()?;
    socket
        .set_nonblocking(true)
        .map_err(|error| anyhow::anyhow!("set discovery socket nonblocking: {error}"))?;
    UdpSocket::from_std(socket).map_err(|error| anyhow::anyhow!("create tokio discovery socket: {error}"))
}

async fn run_discovery_loop(
    socket: UdpSocket,
    device: core_protocol::DeviceDescriptor,
    paths: AppPaths,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    let mut peers: HashMap<String, DiscoveryPeer> = load_discovery_peers(&paths)?
        .into_iter()
        .map(|peer| (peer.device_id.clone(), peer))
        .collect();
    let announce = ProtocolFrame::new(ProtocolMessage::DiscoverAnnounce(device.clone()))
        .encode_json_line()?;
    let broadcast_addr = format!("255.255.255.255:{DISCOVERY_PORT}");
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    let mut buffer = vec![0_u8; 16 * 1024];

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_ok() && *shutdown_rx.borrow() {
                    save_discovery_peers(&paths, &peers.values().cloned().collect::<Vec<_>>())?;
                    break;
                }
            }
            _ = interval.tick() => {
                peers.retain(|_, peer| {
                    unix_time_now_ms().saturating_sub(peer.discovered_at_unix_ms) <= DISCOVERY_PEER_TTL_MS
                });
                save_discovery_peers(&paths, &peers.values().cloned().collect::<Vec<_>>())?;
                if let Ok(config) = load_or_create_config(&paths) {
                    if config.app_role == "controller" && config.controller_service_enabled {
                        let _ = socket.send_to(&announce, &broadcast_addr).await;
                    }
                }
            }
            received = socket.recv_from(&mut buffer) => {
                let (len, addr) = received?;
                let frame = match ProtocolFrame::decode_json_line(&buffer[..len]) {
                    Ok(frame) => frame,
                    Err(_) => continue,
                };
                match frame.message {
                    ProtocolMessage::DiscoverAnnounce(remote) => {
                        if remote.device_id == device.device_id {
                            continue;
                        }
                        peers.insert(
                            remote.device_id.clone(),
                            DiscoveryPeer {
                                device_id: remote.device_id.clone(),
                                display_name: remote.display_name.clone(),
                                platform: remote.platform.clone(),
                                address: addr.ip().to_string(),
                                port: remote.port,
                                fingerprint_sha256: remote.fingerprint_sha256.clone(),
                                certificate_pem: remote.certificate_pem.clone(),
                                discovered_at_unix_ms: unix_time_now_ms(),
                            },
                        );
                        save_discovery_peers(&paths, &peers.values().cloned().collect::<Vec<_>>())?;
                    }
                    ProtocolMessage::PairRequest { device, pairing_code } => {
                        if let Ok(config) = load_or_create_config(&paths) {
                            if config.app_role == "controller" && config.controller_service_enabled {
                                let mut requests = load_pending_pairing_requests(&paths)
                                    .unwrap_or_default()
                                    .into_iter()
                                    .filter(|request| request.device_id != device.device_id)
                                    .collect::<Vec<_>>();
                                requests.push(PendingPairingRequest {
                                    device_id: device.device_id,
                                    display_name: device.display_name,
                                    platform: device.platform,
                                    address: addr.ip().to_string(),
                                    port: device.port,
                                    fingerprint_sha256: device.fingerprint_sha256,
                                    certificate_pem: device.certificate_pem,
                                    pairing_code: pairing_code.value,
                                    received_at_unix_ms: unix_time_now_ms(),
                                });
                                save_pending_pairing_requests(&paths, &requests)?;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

async fn handle_client(stream: TcpStream, shutdown_tx: watch::Sender<bool>) -> Result<()> {
    let pid = std::process::id();
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    let Some(line) = lines.next_line().await? else {
        return Ok(());
    };
    let hello = ProtocolFrame::decode_json_line(line.as_bytes())?;
    let ProtocolMessage::VersionHello(remote) = hello.message else {
        anyhow::bail!("first frame must be version hello");
    };

    let agreed = negotiate_protocol(VersionNegotiation::default(), remote)?;
    let ack = ProtocolFrame::new(ProtocolMessage::VersionHello(VersionNegotiation {
        current: agreed,
        min_supported: agreed,
    }));
    writer.write_all(&ack.encode_json_line()?).await?;
    writer.flush().await?;

    if let Some(line) = lines.next_line().await? {
        let frame = ProtocolFrame::decode_json_line(line.as_bytes())?;
        let response = match frame.message {
            ProtocolMessage::Ping => ProtocolFrame::new(ProtocolMessage::Ready { pid }),
            ProtocolMessage::Shutdown => {
                let _ = shutdown_tx.send(true);
                ProtocolFrame::new(ProtocolMessage::Shutdown)
            }
            other => anyhow::bail!("unexpected protocol message from client: {other:?}"),
        };

        writer.write_all(&response.encode_json_line()?).await?;
        writer.flush().await?;
    }

    Ok(())
}

fn detect_local_host_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("8.8.8.8:80")?;
            socket.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

fn unix_time_now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis()
}
