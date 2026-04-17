use foundation::{
    append_log, export_diagnostic_snapshot, load_or_create_config, save_config, AppConfig, AppPaths,
};
use std::fs;

fn test_paths(name: &str) -> AppPaths {
    let root = std::env::temp_dir()
        .join("deskflow-plus-core-tests")
        .join(name);
    if root.exists() {
        let _ = fs::remove_dir_all(&root);
    }
    AppPaths::from_root(root)
}

#[test]
fn foundation_roundtrip_supports_config_log_and_diagnostic_export() {
    let paths = test_paths("foundation-roundtrip");
    let mut config = load_or_create_config(&paths).expect("load default config");
    config.log_level = "debug".to_string();
    config.clipboard_enabled = false;
    save_config(&paths, &config).expect("save updated config");

    append_log(&paths, "integration log line").expect("append integration log");
    let diagnostic = export_diagnostic_snapshot(&paths, &config).expect("export diagnostic");

    assert!(paths.config_file().exists());
    assert!(paths.log_file().exists());
    assert!(diagnostic.exists());

    let persisted: AppConfig =
        serde_json::from_str(&fs::read_to_string(paths.config_file()).expect("read config file"))
            .expect("parse config file");

    assert_eq!(persisted.log_level, "debug");
    assert!(!persisted.clipboard_enabled);
}

#[test]
fn foundation_layout_includes_security_store_paths() {
    let paths = test_paths("security-layout");
    paths.ensure_layout().expect("ensure layout");

    assert!(paths.security_dir().exists());
    assert_eq!(
        paths
            .device_identity_file()
            .file_name()
            .expect("identity file name"),
        "device-identity.json"
    );
    assert_eq!(
        paths
            .device_certificate_file()
            .file_name()
            .expect("certificate file name"),
        "device-certificate.json"
    );
    assert_eq!(
        paths
            .trust_store_file()
            .file_name()
            .expect("trust store file name"),
        "trust-store.json"
    );
    assert_eq!(
        paths
            .topology_file()
            .file_name()
            .expect("topology file name"),
        "topology.json"
    );
}
