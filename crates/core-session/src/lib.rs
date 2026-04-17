use anyhow::{Context, Result};
use core_protocol::{DeviceDescriptor, PairingCode, ProtocolMessage};
use device_trust::{
    certificate_fingerprint_sha256, load_or_create_certificate, load_or_create_identity, trust_device, validate_trust,
    DeviceCertificate, DeviceIdentity, TrustedDevice,
};
use foundation::AppPaths;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{SocketAddr, UdpSocket};
use uuid::Uuid;

pub const DISCOVERY_PORT: u16 = 24800;
pub const SESSION_PORT: u16 = 24801;
pub const DEFAULT_HEARTBEAT_INTERVAL_MS: u64 = 1000;
pub const DEFAULT_RECONNECT_BACKOFF_MS: u64 = 1500;
pub const DEFAULT_OFFLINE_AFTER_MS: u64 = 5000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManualEndpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryAnnouncement {
    pub device: DeviceDescriptor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PairingDecision {
    Accept,
    Reject { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairingRequest {
    pub requester: DeviceDescriptor,
    pub pairing_code: PairingCode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairingResult {
    pub trusted_device: Option<TrustedDevice>,
    pub message: ProtocolMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionConnectionState {
    Idle,
    Connecting,
    Active {
        session_id: Uuid,
        peer_device_id: Uuid,
        heartbeat_interval_ms: u64,
    },
    Reconnecting {
        peer_device_id: Uuid,
        attempt: u32,
        backoff_ms: u64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ManagedDeviceStatus {
    Online,
    Offline,
    Reconnecting,
    Revoked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedDevice {
    pub device_id: Uuid,
    pub display_name: String,
    pub platform: String,
    pub status: ManagedDeviceStatus,
    pub last_seen_unix_ms: Option<u128>,
    pub reconnect_attempt: u32,
    pub next_retry_after_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryScan {
    pub now_unix_ms: u128,
    pub offline_after_ms: u64,
    pub devices: Vec<ManagedDevice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeviceRepairAction {
    MarkOnline,
    RetryNow,
    Revoke,
}

pub fn manual_endpoint(host: impl Into<String>, port: u16) -> ManualEndpoint {
    ManualEndpoint {
        host: host.into(),
        port,
    }
}

pub fn session_descriptor(
    identity: &DeviceIdentity,
    certificate: &DeviceCertificate,
    endpoint: &ManualEndpoint,
) -> DeviceDescriptor {
    DeviceDescriptor {
        device_id: identity.device_id.to_string(),
        display_name: identity.display_name.clone(),
        platform: identity.platform.clone(),
        address: endpoint.host.clone(),
        port: endpoint.port,
        fingerprint_sha256: certificate.fingerprint_sha256.clone(),
        certificate_pem: certificate.certificate_pem.clone(),
    }
}

pub fn discovery_announce_message(device: DeviceDescriptor) -> ProtocolMessage {
    ProtocolMessage::DiscoverAnnounce(device)
}

pub fn pair_request_message(request: PairingRequest) -> ProtocolMessage {
    ProtocolMessage::PairRequest {
        device: request.requester,
        pairing_code: request.pairing_code,
    }
}

pub fn process_pairing_request(
    paths: &AppPaths,
    request: PairingRequest,
    decision: PairingDecision,
) -> Result<PairingResult> {
    match decision {
        PairingDecision::Accept => {
            let identity = DeviceIdentity {
                device_id: Uuid::parse_str(&request.requester.device_id)
                    .context("parse requester device id")?,
                display_name: request.requester.display_name.clone(),
                platform: request.requester.platform.clone(),
                created_at_unix_ms: 0,
            };
            let certificate = DeviceCertificate {
                subject_device_id: identity.device_id,
                algorithm: "remote".to_string(),
                issued_at_unix_ms: 0,
                fingerprint_sha256: request.requester.fingerprint_sha256.clone(),
                certificate_pem: request.requester.certificate_pem.clone(),
                private_key_pem: String::new(),
            };
            let trusted = trust_device(paths, &identity, &certificate)?;
            Ok(PairingResult {
                trusted_device: Some(trusted),
                message: ProtocolMessage::PairAccept {
                    device_id: request.requester.device_id,
                },
            })
        }
        PairingDecision::Reject { reason } => Ok(PairingResult {
            trusted_device: None,
            message: ProtocolMessage::PairReject {
                device_id: request.requester.device_id,
                reason,
            },
        }),
    }
}

pub fn validate_remote_session(
    paths: &AppPaths,
    descriptor: &DeviceDescriptor,
) -> Result<TrustedDevice> {
    let derived_fingerprint = certificate_fingerprint_sha256(&descriptor.certificate_pem);
    if derived_fingerprint != descriptor.fingerprint_sha256 {
        anyhow::bail!("remote certificate fingerprint does not match descriptor");
    }

    let identity = DeviceIdentity {
        device_id: Uuid::parse_str(&descriptor.device_id).context("parse remote device id")?,
        display_name: descriptor.display_name.clone(),
        platform: descriptor.platform.clone(),
        created_at_unix_ms: 0,
    };
    let certificate = DeviceCertificate {
        subject_device_id: identity.device_id,
        algorithm: "remote".to_string(),
        issued_at_unix_ms: 0,
        fingerprint_sha256: descriptor.fingerprint_sha256.clone(),
        certificate_pem: descriptor.certificate_pem.clone(),
        private_key_pem: String::new(),
    };

    validate_trust(paths, &identity, &certificate)?;
    Ok(TrustedDevice {
        device_id: identity.device_id,
        display_name: identity.display_name,
        platform: identity.platform,
        certificate_fingerprint_sha256: descriptor.fingerprint_sha256.clone(),
        paired_at_unix_ms: 0,
        last_seen_unix_ms: None,
        revoked_at_unix_ms: None,
    })
}

pub fn heartbeat_message(device_id: Uuid, sequence: u64) -> ProtocolMessage {
    ProtocolMessage::SessionHeartbeat {
        device_id: device_id.to_string(),
        sequence,
    }
}

pub fn resume_message(device_id: Uuid, session_id: Uuid) -> ProtocolMessage {
    ProtocolMessage::SessionResume {
        device_id: device_id.to_string(),
        session_id: session_id.to_string(),
    }
}

pub fn next_reconnect_state(peer_device_id: Uuid, attempt: u32) -> SessionConnectionState {
    SessionConnectionState::Reconnecting {
        peer_device_id,
        attempt,
        backoff_ms: DEFAULT_RECONNECT_BACKOFF_MS.saturating_mul(attempt.max(1) as u64),
    }
}

pub fn managed_devices_from_trust_store(
    trust_store: &device_trust::TrustStore,
    now_unix_ms: u128,
    offline_after_ms: u64,
) -> Vec<ManagedDevice> {
    trust_store
        .devices
        .iter()
        .filter(|device| device.revoked_at_unix_ms.is_none())
        .map(|device| {
            let status = if device
                .last_seen_unix_ms
                .is_some_and(|last_seen| now_unix_ms.saturating_sub(last_seen) <= offline_after_ms as u128)
            {
                ManagedDeviceStatus::Online
            } else {
                ManagedDeviceStatus::Offline
            };
            ManagedDevice {
                device_id: device.device_id,
                display_name: device.display_name.clone(),
                platform: device.platform.clone(),
                status,
                last_seen_unix_ms: device.last_seen_unix_ms,
                reconnect_attempt: 0,
                next_retry_after_ms: 0,
            }
        })
        .collect()
}

pub fn recovery_scan_from_trust_store(
    trust_store: &device_trust::TrustStore,
    now_unix_ms: u128,
    offline_after_ms: u64,
) -> RecoveryScan {
    RecoveryScan {
        now_unix_ms,
        offline_after_ms,
        devices: managed_devices_from_trust_store(trust_store, now_unix_ms, offline_after_ms),
    }
}

pub fn schedule_device_reconnect(device: &ManagedDevice, attempt: u32) -> ManagedDevice {
    ManagedDevice {
        status: ManagedDeviceStatus::Reconnecting,
        reconnect_attempt: attempt,
        next_retry_after_ms: DEFAULT_RECONNECT_BACKOFF_MS.saturating_mul(attempt.max(1) as u64),
        ..device.clone()
    }
}

pub fn apply_device_repair(device: &ManagedDevice, action: DeviceRepairAction, now_unix_ms: u128) -> ManagedDevice {
    match action {
        DeviceRepairAction::MarkOnline => ManagedDevice {
            status: ManagedDeviceStatus::Online,
            last_seen_unix_ms: Some(now_unix_ms),
            reconnect_attempt: 0,
            next_retry_after_ms: 0,
            ..device.clone()
        },
        DeviceRepairAction::RetryNow => ManagedDevice {
            status: ManagedDeviceStatus::Reconnecting,
            reconnect_attempt: device.reconnect_attempt.saturating_add(1),
            next_retry_after_ms: 0,
            ..device.clone()
        },
        DeviceRepairAction::Revoke => ManagedDevice {
            status: ManagedDeviceStatus::Revoked,
            reconnect_attempt: 0,
            next_retry_after_ms: 0,
            ..device.clone()
        },
    }
}

pub fn build_server_tls_config(paths: &AppPaths) -> Result<ServerConfig> {
    let identity = load_or_create_identity(paths, "Deskflow-Plus Device")?;
    let certificate = load_or_create_certificate(paths, &identity)?;
    let cert_der = pem_to_certificate_der(&certificate.certificate_pem)?;
    let key_der = pem_to_private_key_der(&certificate.private_key_pem)?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .context("build rustls server config")?;
    Ok(config)
}

pub fn build_client_tls_config(paths: &AppPaths, remote: &DeviceDescriptor) -> Result<ClientConfig> {
    validate_remote_session(paths, remote)?;
    let mut roots = RootCertStore::empty();
    let remote_cert = pem_to_certificate_der(&remote.certificate_pem)?;
    roots
        .add(remote_cert)
        .context("add remote certificate to root store")?;

    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(config)
}

pub fn bind_discovery_socket() -> Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .context("create discovery socket")?;
    socket
        .set_reuse_address(true)
        .context("enable reuse address")?;
    socket
        .set_broadcast(true)
        .context("enable broadcast")?;
    let addr = SocketAddr::from(([0, 0, 0, 0], DISCOVERY_PORT));
    socket
        .bind(&addr.into())
        .context("bind discovery socket")?;
    Ok(socket.into())
}

fn pem_to_certificate_der(pem: &str) -> Result<CertificateDer<'static>> {
    let (item, _) = rustls_pemfile::read_one_from_slice(pem.as_bytes())
        .map_err(|error| anyhow::anyhow!("read certificate pem: {error:?}"))?
        .context("missing certificate pem block")?;
    match item {
        rustls_pemfile::Item::X509Certificate(cert) => Ok(cert),
        other => anyhow::bail!("unexpected pem item for certificate: {other:?}"),
    }
}

fn pem_to_private_key_der(pem: &str) -> Result<PrivateKeyDer<'static>> {
    let (item, _) = rustls_pemfile::read_one_from_slice(pem.as_bytes())
        .map_err(|error| anyhow::anyhow!("read private key pem: {error:?}"))?
        .context("missing private key pem block")?;
    match item {
        rustls_pemfile::Item::Pkcs8Key(key) => Ok(PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.secret_pkcs8_der().to_vec()))),
        other => anyhow::bail!("unexpected pem item for private key: {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use device_trust::{load_or_create_certificate, load_or_create_identity, revoke_trusted_device};
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::thread;

    fn test_paths(name: &str) -> AppPaths {
        let root = std::env::temp_dir()
            .join("deskflow-plus-core-session-tests")
            .join(name);
        if root.exists() {
            let _ = fs::remove_dir_all(&root);
        }
        AppPaths::from_root(root)
    }

    fn local_materials(
        paths: &AppPaths,
        display_name: &str,
        endpoint: &ManualEndpoint,
    ) -> (DeviceIdentity, DeviceCertificate, DeviceDescriptor) {
        let identity = load_or_create_identity(paths, display_name).expect("identity");
        let certificate =
            load_or_create_certificate(paths, &identity).expect("certificate");
        let descriptor = session_descriptor(&identity, &certificate, endpoint);
        (identity, certificate, descriptor)
    }

    #[test]
    fn discovery_announcement_includes_device_metadata() {
        let paths = test_paths("discovery");
        let endpoint = manual_endpoint("192.168.1.50", SESSION_PORT);
        let (_, _, descriptor) = local_materials(&paths, "controller", &endpoint);

        let message = discovery_announce_message(descriptor.clone());
        assert_eq!(message, ProtocolMessage::DiscoverAnnounce(descriptor));
    }

    #[test]
    fn manual_endpoint_is_preserved_in_session_descriptor() {
        let paths = test_paths("manual-endpoint");
        let endpoint = manual_endpoint("10.0.0.12", 34891);
        let (_, _, descriptor) = local_materials(&paths, "client", &endpoint);

        assert_eq!(descriptor.address, "10.0.0.12");
        assert_eq!(descriptor.port, 34891);
    }

    #[test]
    fn accepted_pairing_persists_trust() {
        let local_paths = test_paths("accepted-pairing");
        let remote_paths = test_paths("accepted-pairing-remote");
        let endpoint = manual_endpoint("192.168.0.33", SESSION_PORT);
        let (_, _, remote_descriptor) = local_materials(&remote_paths, "remote-client", &endpoint);

        let request = PairingRequest {
            requester: remote_descriptor.clone(),
            pairing_code: PairingCode {
                value: "654321".into(),
            },
        };
        let result = process_pairing_request(&local_paths, request, PairingDecision::Accept)
            .expect("pair accept");

        assert!(matches!(result.message, ProtocolMessage::PairAccept { .. }));
        let trusted = validate_remote_session(&local_paths, &remote_descriptor)
            .expect("trusted remote session");
        assert_eq!(trusted.device_id.to_string(), remote_descriptor.device_id);
    }

    #[test]
    fn rejected_pairing_does_not_persist_trust() {
        let local_paths = test_paths("rejected-pairing");
        let remote_paths = test_paths("rejected-pairing-remote");
        let endpoint = manual_endpoint("192.168.0.34", SESSION_PORT);
        let (_, _, remote_descriptor) = local_materials(&remote_paths, "remote-client", &endpoint);

        let request = PairingRequest {
            requester: remote_descriptor.clone(),
            pairing_code: PairingCode {
                value: "111111".into(),
            },
        };
        let result = process_pairing_request(
            &local_paths,
            request,
            PairingDecision::Reject {
                reason: "user denied".into(),
            },
        )
        .expect("pair reject");

        assert!(matches!(result.message, ProtocolMessage::PairReject { .. }));
        assert!(validate_remote_session(&local_paths, &remote_descriptor).is_err());
    }

    #[test]
    fn revoked_trust_blocks_session_validation() {
        let local_paths = test_paths("revoke-session");
        let remote_paths = test_paths("revoke-session-remote");
        let endpoint = manual_endpoint("192.168.0.35", SESSION_PORT);
        let (_, _, remote_descriptor) = local_materials(&remote_paths, "remote-client", &endpoint);

        process_pairing_request(
            &local_paths,
            PairingRequest {
                requester: remote_descriptor.clone(),
                pairing_code: PairingCode {
                    value: "222222".into(),
                },
            },
            PairingDecision::Accept,
        )
        .expect("pair accept");

        revoke_trusted_device(
            &local_paths,
            Uuid::parse_str(&remote_descriptor.device_id).expect("parse remote device id"),
        )
        .expect("revoke trusted device");

        let error = validate_remote_session(&local_paths, &remote_descriptor)
            .expect_err("revoked trust should fail");
        assert!(error.to_string().contains("device is not trusted"));
    }

    #[test]
    fn heartbeat_and_resume_messages_encode_expected_ids() {
        let device_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        let heartbeat = heartbeat_message(device_id, 7);
        let resume = resume_message(device_id, session_id);

        assert_eq!(
            heartbeat,
            ProtocolMessage::SessionHeartbeat {
                device_id: device_id.to_string(),
                sequence: 7,
            }
        );
        assert_eq!(
            resume,
            ProtocolMessage::SessionResume {
                device_id: device_id.to_string(),
                session_id: session_id.to_string(),
            }
        );
    }

    #[test]
    fn reconnect_state_applies_backoff_policy() {
        let peer = Uuid::new_v4();
        let reconnect = next_reconnect_state(peer, 3);

        assert_eq!(
            reconnect,
            SessionConnectionState::Reconnecting {
                peer_device_id: peer,
                attempt: 3,
                backoff_ms: DEFAULT_RECONNECT_BACKOFF_MS * 3,
            }
        );
    }

    #[test]
    fn recovery_scan_marks_stale_trusted_devices_offline() {
        let now = 10_000;
        let fresh = TrustedDevice {
            device_id: Uuid::new_v4(),
            display_name: "fresh".into(),
            platform: "windows".into(),
            certificate_fingerprint_sha256: "fresh-fp".into(),
            paired_at_unix_ms: 1,
            last_seen_unix_ms: Some(9_000),
            revoked_at_unix_ms: None,
        };
        let stale = TrustedDevice {
            device_id: Uuid::new_v4(),
            display_name: "stale".into(),
            platform: "macos".into(),
            certificate_fingerprint_sha256: "stale-fp".into(),
            paired_at_unix_ms: 1,
            last_seen_unix_ms: Some(1_000),
            revoked_at_unix_ms: None,
        };
        let revoked = TrustedDevice {
            revoked_at_unix_ms: Some(9_500),
            ..fresh.clone()
        };
        let scan = recovery_scan_from_trust_store(
            &device_trust::TrustStore {
                devices: vec![fresh, stale, revoked],
            },
            now,
            DEFAULT_OFFLINE_AFTER_MS,
        );

        assert_eq!(scan.devices[0].status, ManagedDeviceStatus::Online);
        assert_eq!(scan.devices[1].status, ManagedDeviceStatus::Offline);
        assert_eq!(scan.devices[2].status, ManagedDeviceStatus::Revoked);
    }

    #[test]
    fn reconnect_schedule_and_repair_actions_update_device_state() {
        let device = ManagedDevice {
            device_id: Uuid::new_v4(),
            display_name: "client".into(),
            platform: "windows".into(),
            status: ManagedDeviceStatus::Offline,
            last_seen_unix_ms: Some(100),
            reconnect_attempt: 0,
            next_retry_after_ms: 0,
        };

        let reconnecting = schedule_device_reconnect(&device, 2);
        assert_eq!(reconnecting.status, ManagedDeviceStatus::Reconnecting);
        assert_eq!(reconnecting.next_retry_after_ms, DEFAULT_RECONNECT_BACKOFF_MS * 2);

        let repaired = apply_device_repair(&reconnecting, DeviceRepairAction::MarkOnline, 12_000);
        assert_eq!(repaired.status, ManagedDeviceStatus::Online);
        assert_eq!(repaired.last_seen_unix_ms, Some(12_000));
        assert_eq!(repaired.reconnect_attempt, 0);
    }

    #[test]
    fn tls_server_config_builds_from_persisted_identity_and_certificate() {
        let paths = test_paths("tls-server");
        let config = build_server_tls_config(&paths).expect("build server tls config");
        assert_eq!(config.alpn_protocols.len(), 0);
    }

    #[test]
    fn tls_config_building_completes_under_reasonable_bound() {
        let paths = test_paths("tls-config-performance");
        let started = std::time::Instant::now();

        let config = build_server_tls_config(&paths).expect("build server tls config");

        let elapsed = started.elapsed();
        println!("tls config build elapsed: {elapsed:?}");
        assert_eq!(config.alpn_protocols.len(), 0);
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "tls config build took {elapsed:?}"
        );
    }

    #[test]
    fn fingerprint_mismatch_is_rejected_before_session_establishes() {
        let local_paths = test_paths("fingerprint-mismatch-session");
        let remote_paths = test_paths("fingerprint-mismatch-session-remote");
        let endpoint = manual_endpoint("192.168.0.36", SESSION_PORT);
        let (_, _, mut remote_descriptor) =
            local_materials(&remote_paths, "remote-client", &endpoint);

        process_pairing_request(
            &local_paths,
            PairingRequest {
                requester: remote_descriptor.clone(),
                pairing_code: PairingCode {
                    value: "333333".into(),
                },
            },
            PairingDecision::Accept,
        )
        .expect("pair accept");

        remote_descriptor.fingerprint_sha256 = "tampered".into();
        let error = validate_remote_session(&local_paths, &remote_descriptor)
            .expect_err("fingerprint mismatch must fail");
        assert!(error
            .to_string()
            .contains("remote certificate fingerprint does not match descriptor"));
    }

    #[test]
    fn rustls_client_and_server_configs_can_complete_handshake() {
        let server_paths = test_paths("rustls-server");
        let client_paths = test_paths("rustls-client");
        let server_endpoint = manual_endpoint("127.0.0.1", SESSION_PORT);
        let client_endpoint = manual_endpoint("127.0.0.1", SESSION_PORT);

        let (_, _, server_descriptor) =
            local_materials(&server_paths, "server-device", &server_endpoint);
        let (client_identity, client_certificate, client_descriptor) =
            local_materials(&client_paths, "client-device", &client_endpoint);

        process_pairing_request(
            &server_paths,
            PairingRequest {
                requester: client_descriptor,
                pairing_code: PairingCode {
                    value: "444444".into(),
                },
            },
            PairingDecision::Accept,
        )
        .expect("server trusts client");
        process_pairing_request(
            &client_paths,
            PairingRequest {
                requester: server_descriptor.clone(),
                pairing_code: PairingCode {
                    value: "555555".into(),
                },
            },
            PairingDecision::Accept,
        )
        .expect("client trusts server");

        let server_config = Arc::new(build_server_tls_config(&server_paths).expect("server tls config"));
        let client_config = Arc::new(build_client_tls_config(&client_paths, &server_descriptor).expect("client tls config"));
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind tcp listener");
        let addr = listener.local_addr().expect("listener addr");

        let server_task = thread::spawn(move || -> Result<()> {
            let (stream, _) = listener.accept().context("accept tcp")?;
            let connection =
                rustls::ServerConnection::new(server_config).context("create server connection")?;
            let mut tls = rustls::StreamOwned::new(connection, stream);
            let mut buffer = [0u8; 4];
            tls.read_exact(&mut buffer).context("server read payload")?;
            if &buffer != b"ping" {
                anyhow::bail!("unexpected server payload");
            }
            tls.write_all(b"pong").context("server write payload")?;
            tls.flush().context("server flush payload")?;
            Ok(())
        });

        let started = std::time::Instant::now();
        let stream = TcpStream::connect(addr).expect("connect tcp");
        let connection = rustls::ClientConnection::new(
            client_config,
            rustls::pki_types::ServerName::try_from("server-device").expect("server name"),
        )
        .expect("client connection");
        let mut tls = rustls::StreamOwned::new(connection, stream);
        tls.write_all(b"ping").expect("client write payload");
        tls.flush().expect("client flush payload");
        let mut reply = [0u8; 4];
        tls.read_exact(&mut reply).expect("client read payload");
        assert_eq!(&reply, b"pong");
        let elapsed = started.elapsed();
        println!("rustls loopback session elapsed: {elapsed:?}");

        server_task.join().expect("server task").expect("server result");

        let _ = (client_identity, client_certificate);
    }
}
