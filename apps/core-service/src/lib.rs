use anyhow::{Context, Result};
use core_file_transfer::{checksum_sha256, TransferPlan, TransferProgress};
use core_protocol::{negotiate_protocol, ProtocolFrame, ProtocolMessage, VersionNegotiation};
use core_session::{
    bind_discovery_socket, build_server_tls_config, manual_endpoint, process_pairing_request,
    session_descriptor, PairingDecision, PairingRequest, DISCOVERY_PORT, SESSION_PORT,
};
use core_topology::load_or_create_topology;
use device_trust::{
    default_display_name, load_or_create_certificate, load_or_create_identity,
    update_trusted_device_last_seen,
};
use foundation::{
    append_log, export_diagnostic_snapshot, init_tracing, load_discovery_peers,
    load_or_create_config, load_pending_pairing_requests, save_discovery_peers,
    save_pending_pairing_requests, upsert_cached_peer_descriptor, AppPaths, CachedPeerDescriptor,
    DiscoveryPeer, PendingPairingRequest,
};
use local_ipc::bind_listener;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{BufWriter, Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::watch;
use tracing::info;
use uuid::Uuid;

const DISCOVERY_PEER_TTL_MS: u128 = 8_000;
const MAX_TRANSFER_SESSION_HEADER_BYTES: u64 = 512 * 1024;
const MAX_TRANSFER_CHUNK_HEADER_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct TransferRecord {
    plan: TransferPlan,
    progress: TransferProgress,
    verified_files: usize,
    elapsed_ms: u128,
    created_at_unix_ms: u128,
    #[serde(default = "default_transfer_direction")]
    direction: String,
    #[serde(default)]
    peer_device_id: Option<String>,
    #[serde(default)]
    peer_display_name: Option<String>,
    #[serde(default = "default_delivery_state")]
    delivery_state: String,
    #[serde(default)]
    delivery_message: Option<String>,
    #[serde(default)]
    confirmed_at_unix_ms: Option<u128>,
    #[serde(default)]
    error: Option<String>,
}

fn persist_transfer_stream(
    paths: &AppPaths,
    tls: &mut rustls::StreamOwned<rustls::ServerConnection, std::net::TcpStream>,
    session: TransferSessionHeader,
) -> Result<TransferDeliveryAck> {
    paths.ensure_layout()?;
    let identity = load_or_create_identity(paths, &default_display_name())?;
    let transfer_dir = paths
        .transfers_dir()
        .join(session.record.plan.manifest.transfer_id.to_string());
    fs::create_dir_all(&transfer_dir).context("create transfer artifact directory")?;

    let confirmed_at_unix_ms = unix_time_now_ms();
    let mut record = session.record.clone();
    record.direction = "inbound".into();
    record.peer_device_id = Some(session.source_device_id.clone());
    record.peer_display_name = Some(session.source_display_name.clone());
    record.delivery_state = "received".into();
    record.delivery_message = Some("接收端已落盘并校验完成".into());
    record.confirmed_at_unix_ms = Some(confirmed_at_unix_ms);
    record.verified_files = 0;

    let mut writers: HashMap<Uuid, BufWriter<fs::File>> = HashMap::new();
    let mut verified_files = 0usize;
    let mut transferred_bytes = 0u64;

    loop {
        let mut chunk_length_bytes = [0u8; 8];
        tls.read_exact(&mut chunk_length_bytes)
            .context("read transfer chunk header length")?;
        let chunk_header_len = u64::from_be_bytes(chunk_length_bytes);
        if chunk_header_len == 0 {
            break;
        }
        if chunk_header_len > MAX_TRANSFER_CHUNK_HEADER_BYTES {
            anyhow::bail!("invalid transfer chunk header length: {chunk_header_len}");
        }

        let mut chunk_header_raw = vec![0u8; chunk_header_len as usize];
        tls.read_exact(&mut chunk_header_raw)
            .context("read transfer chunk header payload")?;
        let chunk_header: TransferChunkHeader =
            serde_json::from_slice(&chunk_header_raw).context("parse transfer chunk header")?;

        let expected_file = record
            .plan
            .manifest
            .files
            .iter()
            .find(|file| file.file_id == chunk_header.file_id)
            .ok_or_else(|| anyhow::anyhow!("unknown transfer file {}", chunk_header.file_id))?;
        if expected_file.name != chunk_header.file_name {
            anyhow::bail!("transfer file name mismatch for {}", chunk_header.file_id);
        }

        let mut chunk_bytes = vec![0u8; chunk_header.size_bytes as usize];
        tls.read_exact(&mut chunk_bytes)
            .context("read transfer chunk payload")?;
        let actual_checksum = checksum_sha256(&chunk_bytes);
        if actual_checksum != chunk_header.checksum_sha256 {
            anyhow::bail!(
                "transfer chunk checksum mismatch for {}",
                chunk_header.file_id
            );
        }

        let file_path = transfer_dir.join(&chunk_header.file_name);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("prepare transfer directory {}", parent.display()))?;
        }

        let writer = if let Some(writer) = writers.get_mut(&chunk_header.file_id) {
            writer
        } else {
            let file = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&file_path)
                .with_context(|| format!("open received transfer file {}", file_path.display()))?;
            writers.insert(chunk_header.file_id, BufWriter::new(file));
            writers
                .get_mut(&chunk_header.file_id)
                .expect("writer must exist after insert")
        };
        writer
            .write_all(&chunk_bytes)
            .with_context(|| format!("write received transfer file {}", file_path.display()))?;
        transferred_bytes =
            (transferred_bytes + chunk_header.size_bytes).min(record.plan.manifest.total_bytes);
        if chunk_header.is_last_chunk {
            writer
                .flush()
                .with_context(|| format!("flush received transfer file {}", file_path.display()))?;
            verified_files += 1;
        }
    }

    record.verified_files = verified_files;
    record.progress.transferred_bytes = transferred_bytes;
    record.progress.total_bytes = record.plan.manifest.total_bytes;
    record.progress.status = core_file_transfer::TransferStatus::Completed;
    let summary_path = transfer_dir.join("transfer-record.json");
    let summary = serde_json::to_string_pretty(&record).context("serialize transfer record")?;
    fs::write(&summary_path, summary).context("write transfer record summary")?;

    Ok(TransferDeliveryAck {
        ok: true,
        receiver_device_id: identity.device_id.to_string(),
        receiver_display_name: identity.display_name,
        confirmed_at_unix_ms,
        verified_files,
        total_bytes: record.plan.manifest.total_bytes,
        message: "接收端已落盘并校验完成".into(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct TransferSessionHeader {
    record: TransferRecord,
    source_device_id: String,
    source_display_name: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct TransferArtifactPayload {
    record: TransferRecord,
    source_device_id: String,
    source_display_name: String,
    files: Vec<TransferArtifactFile>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct TransferArtifactFile {
    name: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct TransferChunkHeader {
    transfer_id: uuid::Uuid,
    file_id: uuid::Uuid,
    file_name: String,
    chunk_index: u64,
    offset: u64,
    size_bytes: u64,
    checksum_sha256: String,
    is_last_chunk: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct TransferDeliveryAck {
    ok: bool,
    receiver_device_id: String,
    receiver_display_name: String,
    confirmed_at_unix_ms: u128,
    verified_files: usize,
    total_bytes: u64,
    message: String,
}

pub async fn run_core_service() -> Result<()> {
    let paths = AppPaths::from_runtime_env()?;
    let config = load_or_create_config(&paths)?;
    init_tracing(&config.log_level)?;
    append_log(&paths, "core-service startup")?;
    let identity = load_or_create_identity(&paths, &default_display_name())?;
    let certificate = load_or_create_certificate(&paths, &identity)?;
    let endpoint = manual_endpoint(detect_local_host_ip(), SESSION_PORT);
    let session_device = session_descriptor(&identity, &certificate, &endpoint);
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
    let transfer_listener = std::net::TcpListener::bind(("0.0.0.0", SESSION_PORT))?;
    transfer_listener
        .set_nonblocking(true)
        .map_err(|error| anyhow::anyhow!("set transfer listener nonblocking: {error}"))?;
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
    let transfer_paths = paths.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(error) = run_transfer_listener(transfer_listener, transfer_paths) {
            tracing::error!(?error, "transfer listener failed");
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
    UdpSocket::from_std(socket)
        .map_err(|error| anyhow::anyhow!("create tokio discovery socket: {error}"))
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
    let announce =
        ProtocolFrame::new(ProtocolMessage::DiscoverAnnounce(device.clone())).encode_json_line()?;
    let probe =
        ProtocolFrame::new(ProtocolMessage::DiscoverProbe(device.clone())).encode_json_line()?;
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
                    if config.auto_discovery_enabled {
                        let _ = socket.send_to(&probe, &broadcast_addr).await;
                    }
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
                    ProtocolMessage::DiscoverProbe(remote) => {
                        if remote.device_id == device.device_id {
                            continue;
                        }
                        if let Ok(remote_device_id) = Uuid::parse_str(&remote.device_id) {
                            let _ = update_trusted_device_last_seen(&paths, remote_device_id, unix_time_now_ms());
                        }
                        if let Ok(config) = load_or_create_config(&paths) {
                            if config.app_role == "controller" && config.controller_service_enabled {
                                let _ = socket.send_to(&announce, &addr).await;
                            }
                        }
                    }
                    ProtocolMessage::DiscoverAnnounce(remote) => {
                        if remote.device_id == device.device_id {
                            continue;
                        }
                        if let Ok(remote_device_id) = Uuid::parse_str(&remote.device_id) {
                            let _ = update_trusted_device_last_seen(&paths, remote_device_id, unix_time_now_ms());
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
                    ProtocolMessage::DiscoverWithdraw { device_id } => {
                        peers.remove(&device_id);
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
                    ProtocolMessage::PairAccept { device_id } => {
                        if device_id == device.device_id {
                            continue;
                        }
                        if let Ok(peer) = load_discovery_peers(&paths)?
                            .into_iter()
                            .find(|peer| peer.device_id == device_id)
                            .ok_or_else(|| anyhow::anyhow!("accepted peer not found in discovery snapshot"))
                        {
                            let pairing_request = PairingRequest {
                                requester: discovery_peer_to_descriptor(&peer),
                                pairing_code: core_protocol::PairingCode {
                                    value: format!("auto:accepted:{}", unix_time_now_ms()),
                                },
                            };
                            process_pairing_request(&paths, pairing_request, PairingDecision::Accept)?;
                            upsert_cached_peer_descriptor(
                                &paths,
                                CachedPeerDescriptor {
                                    device_id: peer.device_id.clone(),
                                    display_name: peer.display_name.clone(),
                                    platform: peer.platform.clone(),
                                    address: peer.address.clone(),
                                    port: peer.port,
                                    fingerprint_sha256: peer.fingerprint_sha256.clone(),
                                    certificate_pem: peer.certificate_pem.clone(),
                                    updated_at_unix_ms: unix_time_now_ms(),
                                },
                            )?;
                            let mut config = load_or_create_config(&paths)?;
                            config.app_role = "client".into();
                            config.active_peer_device_id = Some(peer.device_id);
                            config.last_pairing_error = None;
                            foundation::save_config(&paths, &config)?;
                            if let Ok(remote_device_id) = Uuid::parse_str(&device_id) {
                                let _ = update_trusted_device_last_seen(&paths, remote_device_id, unix_time_now_ms());
                            }
                        }
                    }
                    ProtocolMessage::PairReject { reason, .. } => {
                        let mut config = load_or_create_config(&paths)?;
                        if config.app_role == "client" {
                            config.active_peer_device_id = None;
                            config.last_pairing_error = Some(if reason.trim().is_empty() {
                                "主控端已拒绝连接请求".into()
                            } else {
                                format!("主控端已拒绝连接请求：{reason}")
                            });
                            foundation::save_config(&paths, &config)?;
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

fn discovery_peer_to_descriptor(peer: &DiscoveryPeer) -> core_protocol::DeviceDescriptor {
    core_protocol::DeviceDescriptor {
        device_id: peer.device_id.clone(),
        display_name: peer.display_name.clone(),
        platform: peer.platform.clone(),
        address: peer.address.clone(),
        port: peer.port,
        fingerprint_sha256: peer.fingerprint_sha256.clone(),
        certificate_pem: peer.certificate_pem.clone(),
    }
}

fn default_transfer_direction() -> String {
    "inbound".into()
}

fn default_delivery_state() -> String {
    "received".into()
}

fn run_transfer_listener(listener: TcpListener, paths: AppPaths) -> Result<()> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(error) = handle_transfer_connection(stream, &paths) {
                    tracing::error!(?error, "handle transfer connection failed");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(anyhow::anyhow!("accept transfer tcp: {error}")),
        }
    }
}

fn handle_transfer_connection(stream: std::net::TcpStream, paths: &AppPaths) -> Result<()> {
    let server_config = Arc::new(build_server_tls_config(paths)?);
    let connection = rustls::ServerConnection::new(server_config)
        .context("create transfer server tls connection")?;
    let mut tls = rustls::StreamOwned::new(connection, stream);

    let mut length_bytes = [0u8; 8];
    tls.read_exact(&mut length_bytes)
        .context("read transfer session header length")?;
    let payload_len = u64::from_be_bytes(length_bytes);
    if payload_len == 0 || payload_len > MAX_TRANSFER_SESSION_HEADER_BYTES {
        anyhow::bail!("invalid transfer session header length: {payload_len}");
    }
    let mut payload = vec![0u8; payload_len as usize];
    tls.read_exact(&mut payload)
        .context("read transfer session header body")?;

    let session: TransferSessionHeader =
        serde_json::from_slice(&payload).context("parse received transfer session header")?;
    let ack = persist_transfer_stream(paths, &mut tls, session)
        .context("persist received transfer artifacts")?;
    let ack_raw = serde_json::to_vec(&ack).context("serialize transfer ack")?;
    tls.write_all(&(ack_raw.len() as u64).to_be_bytes())
        .context("write transfer ack length")?;
    tls.write_all(&ack_raw).context("write transfer ack")?;
    tls.flush().context("flush transfer ack")?;
    Ok(())
}

#[allow(dead_code)]
fn persist_transfer_artifacts(
    paths: &AppPaths,
    artifact: &TransferArtifactPayload,
) -> Result<TransferDeliveryAck> {
    paths.ensure_layout()?;
    let identity = load_or_create_identity(paths, &default_display_name())?;
    let transfer_dir = paths
        .transfers_dir()
        .join(artifact.record.plan.manifest.transfer_id.to_string());
    fs::create_dir_all(&transfer_dir).context("create transfer artifact directory")?;

    let confirmed_at_unix_ms = unix_time_now_ms();
    let mut record = artifact.record.clone();
    record.direction = "inbound".into();
    record.peer_device_id = Some(artifact.source_device_id.clone());
    record.peer_display_name = Some(artifact.source_display_name.clone());
    record.delivery_state = "received".into();
    record.delivery_message = Some("接收端已落盘并校验完成".into());
    record.confirmed_at_unix_ms = Some(confirmed_at_unix_ms);
    record.verified_files = artifact.files.len();
    let summary_path = transfer_dir.join("transfer-record.json");
    let summary = serde_json::to_string_pretty(&record).context("serialize transfer record")?;
    fs::write(&summary_path, summary).context("write transfer record summary")?;
    for file in &artifact.files {
        fs::write(transfer_dir.join(&file.name), &file.bytes)
            .with_context(|| format!("write received transfer file {}", file.name))?;
    }
    Ok(TransferDeliveryAck {
        ok: true,
        receiver_device_id: identity.device_id.to_string(),
        receiver_display_name: identity.display_name,
        confirmed_at_unix_ms,
        verified_files: artifact.files.len(),
        total_bytes: artifact.record.progress.total_bytes,
        message: "接收端已落盘并校验完成".into(),
    })
}
