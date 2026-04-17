use anyhow::{Context, Result};
use foundation::AppPaths;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub device_id: Uuid,
    pub display_name: String,
    pub platform: String,
    pub created_at_unix_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceCertificate {
    pub subject_device_id: Uuid,
    pub algorithm: String,
    pub issued_at_unix_ms: u128,
    pub fingerprint_sha256: String,
    pub certificate_pem: String,
    pub private_key_pem: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustedDevice {
    pub device_id: Uuid,
    pub display_name: String,
    pub platform: String,
    pub certificate_fingerprint_sha256: String,
    pub paired_at_unix_ms: u128,
    pub last_seen_unix_ms: Option<u128>,
    pub revoked_at_unix_ms: Option<u128>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TrustStore {
    pub devices: Vec<TrustedDevice>,
}

impl TrustStore {
    pub fn upsert(&mut self, device: TrustedDevice) {
        if let Some(existing) = self
            .devices
            .iter_mut()
            .find(|item| item.device_id == device.device_id)
        {
            *existing = device;
            return;
        }

        self.devices.push(device);
    }

    pub fn trusted_device(&self, device_id: Uuid) -> Option<&TrustedDevice> {
        self.devices
            .iter()
            .find(|device| device.device_id == device_id && device.revoked_at_unix_ms.is_none())
    }

    pub fn revoke(&mut self, device_id: Uuid, revoked_at_unix_ms: u128) -> Result<()> {
        let entry = self
            .devices
            .iter_mut()
            .find(|device| device.device_id == device_id)
            .context("trusted device not found")?;
        entry.revoked_at_unix_ms = Some(revoked_at_unix_ms);
        Ok(())
    }
}

pub fn load_or_create_identity(paths: &AppPaths, display_name: &str) -> Result<DeviceIdentity> {
    paths.ensure_layout()?;
    let identity_path = paths.device_identity_file();
    if identity_path.exists() {
        return read_json(&identity_path).context("load device identity");
    }

    let identity = DeviceIdentity {
        device_id: Uuid::new_v4(),
        display_name: display_name.to_string(),
        platform: current_platform(),
        created_at_unix_ms: unix_time_now_ms(),
    };
    write_json(&identity_path, &identity).context("persist device identity")?;
    Ok(identity)
}

pub fn save_identity(paths: &AppPaths, identity: &DeviceIdentity) -> Result<()> {
    paths.ensure_layout()?;
    write_json(&paths.device_identity_file(), identity).context("persist device identity")
}

pub fn default_display_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "Deskflow-Plus Device".to_string())
}

pub fn load_or_create_certificate(
    paths: &AppPaths,
    identity: &DeviceIdentity,
) -> Result<DeviceCertificate> {
    paths.ensure_layout()?;
    let certificate_path = paths.device_certificate_file();
    if certificate_path.exists() {
        return read_json(&certificate_path).context("load device certificate");
    }

    let issued_at_unix_ms = unix_time_now_ms();
    let key_pair = KeyPair::generate().context("generate device certificate key pair")?;
    let mut params = CertificateParams::new(vec![
        identity.display_name.clone(),
        identity.device_id.to_string(),
    ])
    .context("create certificate params")?;
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, identity.display_name.clone());
    params
        .distinguished_name
        .push(DnType::OrganizationName, "Deskflow-Plus");
    params
        .distinguished_name
        .push(DnType::OrganizationalUnitName, identity.device_id.to_string());
    let certificate = params
        .self_signed(&key_pair)
        .context("create self-signed device certificate")?;
    let certificate_pem = certificate.pem();
    let private_key_pem = key_pair.serialize_pem();
    let fingerprint_sha256 = sha256_hex(certificate_pem.as_bytes());

    let certificate = DeviceCertificate {
        subject_device_id: identity.device_id,
        algorithm: "ecdsa-p256-sha256-x509-self-signed".to_string(),
        issued_at_unix_ms,
        fingerprint_sha256,
        certificate_pem,
        private_key_pem,
    };
    write_json(&certificate_path, &certificate).context("persist device certificate")?;
    Ok(certificate)
}

pub fn load_trust_store(paths: &AppPaths) -> Result<TrustStore> {
    paths.ensure_layout()?;
    let trust_store_path = paths.trust_store_file();
    if !trust_store_path.exists() {
        return Ok(TrustStore::default());
    }

    read_json(&trust_store_path).context("load trust store")
}

pub fn save_trust_store(paths: &AppPaths, trust_store: &TrustStore) -> Result<()> {
    paths.ensure_layout()?;
    write_json(&paths.trust_store_file(), trust_store).context("persist trust store")
}

pub fn trust_device(
    paths: &AppPaths,
    identity: &DeviceIdentity,
    certificate: &DeviceCertificate,
) -> Result<TrustedDevice> {
    let mut trust_store = load_trust_store(paths)?;
    let now = unix_time_now_ms();
    let trusted = TrustedDevice {
        device_id: identity.device_id,
        display_name: identity.display_name.clone(),
        platform: identity.platform.clone(),
        certificate_fingerprint_sha256: certificate.fingerprint_sha256.clone(),
        paired_at_unix_ms: now,
        last_seen_unix_ms: Some(now),
        revoked_at_unix_ms: None,
    };
    trust_store.upsert(trusted.clone());
    save_trust_store(paths, &trust_store)?;
    Ok(trusted)
}

pub fn revoke_trusted_device(paths: &AppPaths, device_id: Uuid) -> Result<()> {
    let mut trust_store = load_trust_store(paths)?;
    trust_store.revoke(device_id, unix_time_now_ms())?;
    save_trust_store(paths, &trust_store)
}

pub fn update_trusted_device_last_seen(paths: &AppPaths, device_id: Uuid, seen_at_unix_ms: u128) -> Result<bool> {
    let mut trust_store = load_trust_store(paths)?;
    let Some(device) = trust_store
        .devices
        .iter_mut()
        .find(|device| device.device_id == device_id && device.revoked_at_unix_ms.is_none()) else {
        return Ok(false);
    };
    device.last_seen_unix_ms = Some(seen_at_unix_ms);
    save_trust_store(paths, &trust_store)?;
    Ok(true)
}

pub fn validate_trust(
    paths: &AppPaths,
    identity: &DeviceIdentity,
    certificate: &DeviceCertificate,
) -> Result<()> {
    let trust_store = load_trust_store(paths)?;
    let Some(trusted) = trust_store.trusted_device(identity.device_id) else {
        anyhow::bail!("device is not trusted");
    };

    if trusted.certificate_fingerprint_sha256 != certificate.fingerprint_sha256 {
        anyhow::bail!("certificate fingerprint mismatch");
    }

    Ok(())
}

pub fn certificate_fingerprint_sha256(certificate_pem: &str) -> String {
    sha256_hex(certificate_pem.as_bytes())
}

fn current_platform() -> String {
    std::env::consts::OS.to_string()
}

fn unix_time_now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis()
}

fn sha256_hex(input: &[u8]) -> String {
    let digest = Sha256::digest(input);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_json<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))
}

fn write_json<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let raw = serde_json::to_string_pretty(value).context("serialize json")?;
    fs::write(path, raw).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_paths(name: &str) -> AppPaths {
        let root = std::env::temp_dir()
            .join("deskflow-plus-device-trust-tests")
            .join(name);
        if root.exists() {
            let _ = fs::remove_dir_all(&root);
        }
        AppPaths::from_root(root)
    }

    #[test]
    fn identity_is_persisted_once_created() {
        let paths = test_paths("identity-persist");
        let first = load_or_create_identity(&paths, "controller").expect("create identity");
        let second = load_or_create_identity(&paths, "other-name").expect("reload identity");

        assert_eq!(first, second);
        assert!(paths.device_identity_file().exists());
    }

    #[test]
    fn certificate_is_persisted_and_bound_to_identity() {
        let paths = test_paths("certificate-persist");
        let identity = load_or_create_identity(&paths, "controller").expect("identity");
        let certificate =
            load_or_create_certificate(&paths, &identity).expect("create certificate");
        let reloaded =
            load_or_create_certificate(&paths, &identity).expect("reload certificate");

        assert_eq!(certificate, reloaded);
        assert_eq!(certificate.subject_device_id, identity.device_id);
        assert!(!certificate.fingerprint_sha256.is_empty());
        assert!(certificate.certificate_pem.contains("BEGIN CERTIFICATE"));
        assert!(certificate.private_key_pem.contains("BEGIN"));
        assert!(paths.device_certificate_file().exists());
    }

    #[test]
    fn trust_store_supports_trust_validation_and_revocation() {
        let paths = test_paths("trust-store");
        let identity = load_or_create_identity(&paths, "client-a").expect("identity");
        let certificate =
            load_or_create_certificate(&paths, &identity).expect("create certificate");

        trust_device(&paths, &identity, &certificate).expect("trust device");
        validate_trust(&paths, &identity, &certificate).expect("validate trust");

        revoke_trusted_device(&paths, identity.device_id).expect("revoke device");
        let error = validate_trust(&paths, &identity, &certificate).expect_err("trust revoked");
        assert!(error.to_string().contains("device is not trusted"));
    }

    #[test]
    fn trust_validation_rejects_certificate_rotation_without_repairing() {
        let paths = test_paths("fingerprint-mismatch");
        let identity = load_or_create_identity(&paths, "client-b").expect("identity");
        let certificate =
            load_or_create_certificate(&paths, &identity).expect("create certificate");
        trust_device(&paths, &identity, &certificate).expect("trust device");

        let rotated = DeviceCertificate {
            fingerprint_sha256: "rotated-fingerprint".to_string(),
            ..certificate.clone()
        };

        let error = validate_trust(&paths, &identity, &rotated).expect_err("fingerprint mismatch");
        assert!(error.to_string().contains("certificate fingerprint mismatch"));
    }

    #[test]
    fn certificate_generation_completes_under_reasonable_bound() {
        let paths = test_paths("certificate-generation-performance");
        let identity = load_or_create_identity(&paths, "perf-device").expect("identity");
        let started = std::time::Instant::now();

        let certificate =
            load_or_create_certificate(&paths, &identity).expect("create certificate");

        let elapsed = started.elapsed();
        println!("certificate generation elapsed: {elapsed:?}");
        assert!(!certificate.certificate_pem.is_empty());
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "certificate generation took {elapsed:?}"
        );
    }
}
