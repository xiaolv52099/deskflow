use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub const QUALIFIER: &str = "org";
pub const ORGANIZATION: &str = "Deskflow";
pub const APPLICATION: &str = "Deskflow-Plus";
pub const CONFIG_FILE_NAME: &str = "config.json";
pub const LOG_FILE_NAME: &str = "core-service.log";
pub const DIAGNOSTIC_FILE_NAME: &str = "diagnostic.json";
pub const DEVICE_IDENTITY_FILE_NAME: &str = "device-identity.json";
pub const DEVICE_CERTIFICATE_FILE_NAME: &str = "device-certificate.json";
pub const TRUST_STORE_FILE_NAME: &str = "trust-store.json";
pub const TOPOLOGY_FILE_NAME: &str = "topology.json";
pub const DISCOVERY_FILE_NAME: &str = "discovery.json";
pub const PAIRING_REQUESTS_FILE_NAME: &str = "pairing-requests.json";
pub const TRANSFERS_DIR_NAME: &str = "transfers";
pub const DATA_ROOT_ENV_VAR: &str = "DESKFLOW_PLUS_DATA_ROOT";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputTuningConfig {
    pub pointer_speed_multiplier: f64,
    pub wheel_speed_multiplier: f64,
    pub wheel_smoothing_factor: f64,
}

impl Default for InputTuningConfig {
    fn default() -> Self {
        Self {
            pointer_speed_multiplier: 1.0,
            wheel_speed_multiplier: 1.0,
            wheel_smoothing_factor: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    pub log_level: String,
    pub auto_discovery_enabled: bool,
    pub clipboard_enabled: bool,
    #[serde(default)]
    pub input_tuning: InputTuningConfig,
    #[serde(default = "default_app_role")]
    pub app_role: String,
    #[serde(default)]
    pub controller_service_enabled: bool,
    #[serde(default)]
    pub current_pairing_code: Option<String>,
    #[serde(default)]
    pub active_peer_device_id: Option<String>,
    #[serde(default)]
    pub last_pairing_error: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            auto_discovery_enabled: true,
            clipboard_enabled: true,
            input_tuning: InputTuningConfig::default(),
            app_role: default_app_role(),
            controller_service_enabled: false,
            current_pairing_code: None,
            active_peer_device_id: None,
            last_pairing_error: None,
        }
    }
}

fn default_app_role() -> String {
    "controller".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticSnapshot {
    pub config_path: PathBuf,
    pub log_path: PathBuf,
    pub log_level: String,
    pub auto_discovery_enabled: bool,
    pub clipboard_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryPeer {
    pub device_id: String,
    pub display_name: String,
    pub platform: String,
    pub address: String,
    pub port: u16,
    pub fingerprint_sha256: String,
    pub certificate_pem: String,
    pub discovered_at_unix_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingPairingRequest {
    pub device_id: String,
    pub display_name: String,
    pub platform: String,
    pub address: String,
    pub port: u16,
    pub fingerprint_sha256: String,
    pub certificate_pem: String,
    pub pairing_code: String,
    pub received_at_unix_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticMetric {
    pub name: String,
    pub value: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExtendedDiagnosticSnapshot {
    pub generated_at_unix_ms: u128,
    pub config: AppConfig,
    pub root_path: PathBuf,
    pub log_path: PathBuf,
    pub topology_path: PathBuf,
    pub trust_store_path: PathBuf,
    pub recent_log_lines: Vec<String>,
    pub metrics: Vec<DiagnosticMetric>,
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn from_project_dirs() -> Result<Self> {
        let project_dirs = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
            .context("resolve project directories for Deskflow-Plus")?;
        Ok(Self {
            root: project_dirs.data_local_dir().to_path_buf(),
        })
    }

    pub fn from_runtime_env() -> Result<Self> {
        if let Some(root) = std::env::var_os(DATA_ROOT_ENV_VAR) {
            return Ok(Self::from_root(root));
        }

        Self::from_project_dirs()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config_dir(&self) -> PathBuf {
        self.root.join("config")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn diagnostic_dir(&self) -> PathBuf {
        self.root.join("diagnostic")
    }

    pub fn security_dir(&self) -> PathBuf {
        self.root.join("security")
    }

    pub fn topology_dir(&self) -> PathBuf {
        self.root.join("topology")
    }

    pub fn discovery_dir(&self) -> PathBuf {
        self.root.join("discovery")
    }

    pub fn transfers_dir(&self) -> PathBuf {
        self.root.join(TRANSFERS_DIR_NAME)
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir().join(CONFIG_FILE_NAME)
    }

    pub fn log_file(&self) -> PathBuf {
        self.logs_dir().join(LOG_FILE_NAME)
    }

    pub fn diagnostic_file(&self) -> PathBuf {
        self.diagnostic_dir().join(DIAGNOSTIC_FILE_NAME)
    }

    pub fn device_identity_file(&self) -> PathBuf {
        self.security_dir().join(DEVICE_IDENTITY_FILE_NAME)
    }

    pub fn device_certificate_file(&self) -> PathBuf {
        self.security_dir().join(DEVICE_CERTIFICATE_FILE_NAME)
    }

    pub fn trust_store_file(&self) -> PathBuf {
        self.security_dir().join(TRUST_STORE_FILE_NAME)
    }

    pub fn topology_file(&self) -> PathBuf {
        self.topology_dir().join(TOPOLOGY_FILE_NAME)
    }

    pub fn discovery_file(&self) -> PathBuf {
        self.discovery_dir().join(DISCOVERY_FILE_NAME)
    }

    pub fn pairing_requests_file(&self) -> PathBuf {
        self.discovery_dir().join(PAIRING_REQUESTS_FILE_NAME)
    }

    pub fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(self.config_dir()).context("create config directory")?;
        fs::create_dir_all(self.logs_dir()).context("create logs directory")?;
        fs::create_dir_all(self.diagnostic_dir()).context("create diagnostic directory")?;
        fs::create_dir_all(self.security_dir()).context("create security directory")?;
        fs::create_dir_all(self.topology_dir()).context("create topology directory")?;
        fs::create_dir_all(self.discovery_dir()).context("create discovery directory")?;
        fs::create_dir_all(self.transfers_dir()).context("create transfers directory")?;
        Ok(())
    }
}

pub fn load_or_create_config(paths: &AppPaths) -> Result<AppConfig> {
    paths.ensure_layout()?;
    let config_path = paths.config_file();

    if config_path.exists() {
        let raw = fs::read_to_string(&config_path).context("read config file")?;
        let config = serde_json::from_str(&raw).context("parse config file")?;
        return Ok(config);
    }

    let config = AppConfig::default();
    save_config(paths, &config)?;
    Ok(config)
}

pub fn save_config(paths: &AppPaths, config: &AppConfig) -> Result<()> {
    paths.ensure_layout()?;
    let raw = serde_json::to_string_pretty(config).context("serialize app config")?;
    fs::write(paths.config_file(), raw).context("write config file")?;
    Ok(())
}

pub fn append_log(paths: &AppPaths, message: &str) -> Result<()> {
    paths.ensure_layout()?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.log_file())
        .context("open log file")?;
    writeln!(file, "{message}").context("append log line")?;
    Ok(())
}

pub fn export_diagnostic_snapshot(paths: &AppPaths, config: &AppConfig) -> Result<PathBuf> {
    paths.ensure_layout()?;
    let snapshot = DiagnosticSnapshot {
        config_path: paths.config_file(),
        log_path: paths.log_file(),
        log_level: config.log_level.clone(),
        auto_discovery_enabled: config.auto_discovery_enabled,
        clipboard_enabled: config.clipboard_enabled,
    };

    let raw = serde_json::to_string_pretty(&snapshot).context("serialize diagnostic snapshot")?;
    let path = paths.diagnostic_file();
    fs::write(&path, raw).context("write diagnostic snapshot")?;
    Ok(path)
}

pub fn export_extended_diagnostic_snapshot(
    paths: &AppPaths,
    config: &AppConfig,
    metrics: Vec<DiagnosticMetric>,
    recent_log_limit: usize,
) -> Result<PathBuf> {
    paths.ensure_layout()?;
    let snapshot = ExtendedDiagnosticSnapshot {
        generated_at_unix_ms: unix_time_now_ms(),
        config: config.clone(),
        root_path: paths.root().to_path_buf(),
        log_path: paths.log_file(),
        topology_path: paths.topology_file(),
        trust_store_path: paths.trust_store_file(),
        recent_log_lines: read_recent_log_lines(paths, recent_log_limit)?,
        metrics,
    };

    let raw = serde_json::to_string_pretty(&snapshot).context("serialize extended diagnostic snapshot")?;
    let path = paths.diagnostic_dir().join("diagnostic-extended.json");
    fs::write(&path, raw).context("write extended diagnostic snapshot")?;
    Ok(path)
}

pub fn load_discovery_peers(paths: &AppPaths) -> Result<Vec<DiscoveryPeer>> {
    paths.ensure_layout()?;
    let path = paths.discovery_file();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw = fs::read_to_string(&path).context("read discovery snapshot")?;
    serde_json::from_str(&raw).context("parse discovery snapshot")
}

pub fn save_discovery_peers(paths: &AppPaths, peers: &[DiscoveryPeer]) -> Result<()> {
    paths.ensure_layout()?;
    let raw = serde_json::to_string_pretty(peers).context("serialize discovery snapshot")?;
    fs::write(paths.discovery_file(), raw).context("write discovery snapshot")?;
    Ok(())
}

pub fn load_pending_pairing_requests(paths: &AppPaths) -> Result<Vec<PendingPairingRequest>> {
    paths.ensure_layout()?;
    let path = paths.pairing_requests_file();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw = fs::read_to_string(&path).context("read pending pairing requests")?;
    serde_json::from_str(&raw).context("parse pending pairing requests")
}

pub fn save_pending_pairing_requests(
    paths: &AppPaths,
    requests: &[PendingPairingRequest],
) -> Result<()> {
    paths.ensure_layout()?;
    let raw = serde_json::to_string_pretty(requests).context("serialize pending pairing requests")?;
    fs::write(paths.pairing_requests_file(), raw).context("write pending pairing requests")?;
    Ok(())
}

pub fn read_recent_log_lines(paths: &AppPaths, limit: usize) -> Result<Vec<String>> {
    if limit == 0 || !paths.log_file().exists() {
        return Ok(Vec::new());
    }

    let file = fs::File::open(paths.log_file()).context("open log file for tail")?;
    let reader = BufReader::new(file);
    let mut lines = Vec::new();
    for line in reader.lines() {
        lines.push(line.context("read log line")?);
        if lines.len() > limit {
            lines.remove(0);
        }
    }
    Ok(lines)
}

pub fn init_tracing(log_level: &str) -> Result<()> {
    let filter = tracing_subscriber::EnvFilter::try_new(log_level)
        .or_else(|_| tracing_subscriber::EnvFilter::try_new("info"))
        .context("create tracing env filter")?;

    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
    Ok(())
}

fn unix_time_now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_paths(name: &str) -> AppPaths {
        let root = std::env::temp_dir()
            .join("deskflow-plus-tests")
            .join(name);
        if root.exists() {
            let _ = fs::remove_dir_all(&root);
        }
        AppPaths::from_root(root)
    }

    #[test]
    fn creates_default_config_when_missing() {
        let paths = test_paths("default-config");
        let config = load_or_create_config(&paths).expect("load config");
        assert_eq!(config, AppConfig::default());
        assert!(paths.config_file().exists());
    }

    #[test]
    fn appends_log_and_exports_diagnostic_snapshot() {
        let paths = test_paths("diagnostic");
        let config = load_or_create_config(&paths).expect("load config");

        append_log(&paths, "hello from test").expect("append log");
        let diagnostic = export_diagnostic_snapshot(&paths, &config).expect("export diagnostic");

        assert!(paths.log_file().exists());
        assert!(diagnostic.exists());
    }

    #[test]
    fn extended_diagnostic_includes_recent_logs_and_metrics() {
        let paths = test_paths("extended-diagnostic");
        let config = load_or_create_config(&paths).expect("load config");
        append_log(&paths, "line-1").expect("append first log");
        append_log(&paths, "line-2").expect("append second log");
        append_log(&paths, "line-3").expect("append third log");

        let diagnostic = export_extended_diagnostic_snapshot(
            &paths,
            &config,
            vec![DiagnosticMetric {
                name: "latency".into(),
                value: "2ms".into(),
                status: "passed".into(),
            }],
            2,
        )
        .expect("export extended diagnostic");

        let raw = fs::read_to_string(diagnostic).expect("read extended diagnostic");
        let snapshot: ExtendedDiagnosticSnapshot =
            serde_json::from_str(&raw).expect("parse extended diagnostic");
        assert_eq!(snapshot.recent_log_lines, vec!["line-2", "line-3"]);
        assert_eq!(snapshot.metrics[0].name, "latency");
    }

    #[test]
    fn discovery_snapshot_roundtrip() {
        let paths = test_paths("discovery-snapshot");
        let peers = vec![DiscoveryPeer {
            device_id: "device-1".into(),
            display_name: "Deskflow Client".into(),
            platform: "windows".into(),
            address: "192.168.1.20".into(),
            port: 24801,
            fingerprint_sha256: "abc".into(),
            certificate_pem: "-----BEGIN CERTIFICATE-----\nabc\n-----END CERTIFICATE-----".into(),
            discovered_at_unix_ms: 100,
        }];

        save_discovery_peers(&paths, &peers).expect("save discovery peers");
        let loaded = load_discovery_peers(&paths).expect("load discovery peers");
        assert_eq!(loaded, peers);
    }
}
