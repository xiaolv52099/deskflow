#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{Context as AnyhowContext, Result};
use core_clipboard::{ClipboardSyncEngine, ImageClipboardFormat};
use core_file_transfer::{
    approve_transfer, checksum_sha256, plan_transfer, TransferFileDescriptor, TransferPlan,
    TransferProgress,
};
use core_input::{
    current_platform_input_status, sample_cursor_position, InputTuningProfile, PlatformInputStatus,
};
use core_protocol::{DeviceDescriptor, PairingCode};
use core_service::run_core_service;
use core_session::{
    apply_device_repair, build_client_tls_config, managed_devices_from_trust_store,
    manual_endpoint, process_pairing_request, schedule_device_reconnect, session_descriptor,
    DeviceRepairAction, ManagedDevice, ManagedDeviceStatus, PairingDecision, PairingRequest,
    DEFAULT_OFFLINE_AFTER_MS, DISCOVERY_PORT, SESSION_PORT,
};
use core_topology::{
    apply_hot_update, load_or_create_topology, save_topology, GridPosition, TopologyLayout,
};
use device_trust::{
    default_display_name, load_or_create_certificate, load_or_create_identity, load_trust_store,
    revoke_trusted_device, save_identity,
};
use foundation::{
    append_log, export_extended_diagnostic_snapshot, load_cached_peer_descriptors,
    load_discovery_peers, load_or_create_config, load_pending_pairing_requests,
    read_recent_log_lines, remove_cached_peer_descriptor, save_config, save_discovery_peers,
    save_pending_pairing_requests, upsert_cached_peer_descriptor, AppConfig, AppPaths,
    CachedPeerDescriptor, DiagnosticMetric, DiscoveryPeer, DATA_ROOT_ENV_VAR,
};
use local_ipc::{send_command, CoreToUiEvent, UiToCoreCommand};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::menu::{MenuBuilder, MenuEvent, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use tokio::runtime::Runtime;
use uuid::Uuid;
#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HWND};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex};
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

const DISCOVERY_PEER_TTL_MS: u128 = 8_000;
const TRANSFER_IO_TIMEOUT_SECS: u64 = 120;
const MAX_TRANSFER_ACK_BYTES: u64 = 64 * 1024;
#[cfg(windows)]
const SINGLE_INSTANCE_MUTEX_NAME: &str = "DeskflowPlus.SingleInstance";
#[cfg(windows)]
const MAIN_WINDOW_TITLE: &str = "Deskflow-Plus";

struct AppState {
    runtime: Runtime,
    owns_core_service: bool,
    core_join: Mutex<Option<std::thread::JoinHandle<()>>>,
    data_paths: AppPaths,
    config: Mutex<AppConfig>,
    health: Mutex<AppHealth>,
    topology: Mutex<TopologyLayout>,
    clipboard: Mutex<ClipboardSyncEngine>,
    tuning: Mutex<InputTuningProfile>,
    transfer_records: Mutex<Vec<TransferRecord>>,
    managed_devices: Mutex<Vec<ManagedDevice>>,
    tray_status: Mutex<String>,
    boot_error: Arc<Mutex<Option<String>>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct PendingDeviceRequest {
    display_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct AppHealth {
    protocol_version: u32,
    discovery_port: u16,
    session_port: u16,
    topology_version: u64,
    clipboard_enabled: bool,
    auto_discovery_enabled: bool,
    app_role: String,
    controller_service_enabled: bool,
    active_peer_device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct ConnectionStateDto {
    app_role: String,
    controller_service_enabled: bool,
    current_pairing_code: Option<String>,
    active_peer_device_id: Option<String>,
    active_peer_display_name: Option<String>,
    active_peer_state: String,
    last_pairing_error: Option<String>,
    pending_pairing_requests: Vec<PendingPairingRequestDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct PendingPairingRequestDto {
    device_id: String,
    display_name: String,
    platform: String,
    address: String,
    port: u16,
    pairing_code: String,
    received_at_unix_ms: u128,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct SetAppRoleRequest {
    role: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ConfirmPendingPairingRequest {
    device_id: String,
    accept: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct GridPositionDto {
    x: i32,
    y: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct TopologyDeviceDto {
    device_id: String,
    display_name: String,
    position: Option<GridPositionDto>,
    status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct TopologySnapshot {
    version: u64,
    grid_width: i32,
    grid_height: i32,
    controller_device_id: String,
    devices: Vec<TopologyDeviceDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct RuntimeOverview {
    health: AppHealth,
    input_status: PlatformInputStatus,
    cursor_sample: Option<core_input::CursorSample>,
    clipboard_enabled: bool,
    tuning: InputTuningProfile,
    tray_status: String,
    boot_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct ClipboardStateDto {
    enabled: bool,
    local_device_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ClipboardTextRequest {
    text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ClipboardImageRequest {
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct ClipboardImageDto {
    width: u32,
    height: u32,
    bytes_len: usize,
    checksum_sha256: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct PairingOfferDto {
    display_name: String,
    endpoint_host: String,
    session_port: u16,
    pairing_code: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct PairingImportRequest {
    payload: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ManualPairingConnectRequest {
    host: String,
    port: u16,
    pairing_code: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct DiscoveryTrustRequest {
    device_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct InputTuningRequest {
    pointer_speed_multiplier: f64,
    wheel_speed_multiplier: f64,
    wheel_smoothing_factor: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct InputTuningDto {
    pointer_speed_multiplier: f64,
    wheel_speed_multiplier: f64,
    wheel_smoothing_factor: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct TransferPlanRequest {
    target_device_id: String,
    files: Vec<TransferFileRequest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct TransferFileRequest {
    name: String,
    path: String,
    #[allow(dead_code)]
    size_bytes: u64,
}

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
    #[serde(default)]
    artifacts: Vec<TransferArtifactRef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct TransferArtifactRequest {
    transfer_id: String,
    file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct TransferArtifactRef {
    file_name: String,
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct TransferSessionHeader {
    record: TransferRecord,
    source_device_id: String,
    source_display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct TransferArtifactFile {
    name: String,
    source_path: PathBuf,
    size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct TransferChunkHeader {
    transfer_id: Uuid,
    file_id: Uuid,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct DeviceManagementSnapshot {
    devices: Vec<ManagedDevice>,
    offline_after_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct DeviceRepairRequest {
    device_id: String,
    action: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct DiagnosticExportDto {
    path: String,
    metrics: Vec<DiagnosticMetric>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct DeviceProfileDto {
    device_id: String,
    display_name: String,
    platform: String,
    lan_ip: String,
    session_port: u16,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct UpdateDeviceProfileRequest {
    display_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct PairingConnectResultDto {
    imported_payload: String,
    pairing_code: String,
    endpoint_host: String,
    session_port: u16,
    device_management: DeviceManagementSnapshot,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct LogPreviewDto {
    log_path: String,
    lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct SelectedTransferFileDto {
    name: String,
    path: String,
    size_bytes: u64,
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}

fn main() {
    let _single_instance_guard = match acquire_single_instance_guard() {
        Ok(Some(guard)) => Some(guard),
        Ok(None) => return,
        Err(error) => panic!("failed to initialize single-instance guard: {error:#}"),
    };

    init_tracing();

    let state = initialize_app_state().expect("failed to initialize app state");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = reveal_main_window(app);
        }))
        .manage(state)
        .setup(|app| {
            install_tray(app)?;
            reveal_main_window(app.handle())?;
            Ok(())
        })
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(handle_tray_event)
        .invoke_handler(tauri::generate_handler![
            get_app_health,
            get_runtime_overview,
            get_connection_state,
            set_app_role,
            set_controller_service_enabled,
            submit_discovery_pairing_request,
            respond_to_pending_pairing,
            disconnect_active_peer,
            get_topology_snapshot,
            add_pending_topology_device,
            place_topology_device,
            mark_topology_device_offline,
            get_clipboard_state,
            set_clipboard_enabled,
            simulate_clipboard_broadcast,
            simulate_image_clipboard_broadcast,
            get_device_profile,
            update_device_profile,
            create_pairing_offer,
            accept_pairing_payload,
            connect_to_manual_endpoint,
            list_discovered_peers,
            trust_discovered_peer,
            get_input_tuning,
            update_input_tuning,
            describe_transfer_files,
            create_transfer_plan,
            list_transfer_plans,
            get_transfer_artifact_path,
            reveal_transfer_artifact_location,
            get_device_management_snapshot,
            repair_managed_device,
            get_log_preview,
            export_diagnostics
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run Deskflow-Plus");
}

#[tauri::command]
fn describe_transfer_files(paths: Vec<String>) -> Result<Vec<SelectedTransferFileDto>, String> {
    let mut selected = Vec::new();
    for path in paths {
        let path = PathBuf::from(path);
        let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
        if !metadata.is_file() {
            continue;
        }
        let Some(name_os) = path.file_name() else {
            continue;
        };
        let Some(name) = name_os.to_str() else {
            continue;
        };
        selected.push(SelectedTransferFileDto {
            name: name.to_string(),
            path: path.display().to_string(),
            size_bytes: metadata.len(),
        });
    }

    Ok(selected)
}

struct SingleInstanceGuard {
    #[cfg(windows)]
    handle: isize,
}

fn acquire_single_instance_guard() -> Result<Option<SingleInstanceGuard>> {
    #[cfg(windows)]
    {
        if reveal_existing_native_window() {
            return Ok(None);
        }

        let mutex_name = encode_wide(SINGLE_INSTANCE_MUTEX_NAME);
        let handle = unsafe { CreateMutexW(std::ptr::null(), true.into(), mutex_name.as_ptr()) };
        if handle.is_null() {
            anyhow::bail!("CreateMutexW returned null");
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            let _ = reveal_existing_native_window();
            unsafe {
                CloseHandle(handle);
            }
            return Ok(None);
        }

        return Ok(Some(SingleInstanceGuard {
            handle: handle as isize,
        }));
    }

    #[cfg(not(windows))]
    {
        Ok(Some(SingleInstanceGuard {}))
    }
}

#[cfg(windows)]
fn reveal_existing_native_window() -> bool {
    unsafe {
        let title = encode_wide(MAIN_WINDOW_TITLE);
        let hwnd: HWND = FindWindowW(std::ptr::null(), title.as_ptr());
        if !hwnd.is_null() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
            return true;
        }
    }
    false
}

#[cfg(windows)]
fn encode_wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            ReleaseMutex(self.handle as _);
            CloseHandle(self.handle as _);
        }
    }
}

fn initialize_app_state() -> Result<AppState> {
    let data_root = AppPaths::from_project_dirs()?.root().to_path_buf();
    let runtime = Runtime::new().context("create tokio runtime for app-desktop")?;
    let paths = AppPaths::from_root(data_root.clone());
    let config = load_or_create_config(&paths)?;
    let identity = load_or_create_identity(&paths, &default_display_name())?;
    let topology = load_or_create_topology(&paths, identity.device_id, &identity.display_name)?;
    let trust_store = load_trust_store(&paths)?;
    let managed_devices = managed_devices_from_trust_store(
        &trust_store,
        unix_time_now_ms(),
        DEFAULT_OFFLINE_AFTER_MS,
    );
    let boot_error = Arc::new(Mutex::new(None));
    let (owns_core_service, join) = match runtime.block_on(wait_until_ready_with_retry(2, 50)) {
        Ok(_) => (false, None),
        Err(_) => {
            let boot_error_thread = Arc::clone(&boot_error);
            let data_root_for_thread = data_root.clone();
            let join = std::thread::spawn(move || {
                let result = (|| -> Result<()> {
                    std::env::set_var(DATA_ROOT_ENV_VAR, &data_root_for_thread);
                    let runtime = tokio::runtime::Runtime::new()
                        .context("create embedded core-service runtime")?;
                    runtime.block_on(run_core_service())
                })();

                if let Err(error) = result {
                    let failure_paths = AppPaths::from_root(data_root_for_thread.clone());
                    let _ = append_log(
                        &failure_paths,
                        &format!("embedded core-service startup failed: {error:#}"),
                    );
                    if let Ok(mut slot) = boot_error_thread.lock() {
                        *slot = Some(format!("{error:#}"));
                    }
                }
            });
            (true, Some(join))
        }
    };
    let protocol_version = match runtime.block_on(wait_until_ready_with_retry(40, 100)) {
        Ok(CoreToUiEvent::Ready {
            protocol_version, ..
        }) => protocol_version,
        Ok(other) => {
            if let Ok(mut slot) = boot_error.lock() {
                *slot = Some(format!("unexpected readiness event: {other:?}"));
            }
            core_protocol::CURRENT_PROTOCOL_VERSION
        }
        Err(error) => {
            let _ = append_log(
                &paths,
                &format!("core-service readiness failed during app startup: {error:#}"),
            );
            if let Ok(mut slot) = boot_error.lock() {
                *slot = Some(format!("{error:#}"));
            }
            core_protocol::CURRENT_PROTOCOL_VERSION
        }
    };
    if protocol_version == core_protocol::CURRENT_PROTOCOL_VERSION {
        if let Ok(mut slot) = boot_error.lock() {
            if slot.is_some() && runtime.block_on(wait_until_ready()).is_ok() {
                *slot = None;
            }
        }
    }

    Ok(AppState {
        runtime,
        owns_core_service,
        core_join: Mutex::new(join),
        data_paths: paths,
        config: Mutex::new(config.clone()),
        health: Mutex::new(AppHealth {
            protocol_version,
            discovery_port: DISCOVERY_PORT,
            session_port: SESSION_PORT,
            topology_version: topology.version,
            clipboard_enabled: config.clipboard_enabled,
            auto_discovery_enabled: config.auto_discovery_enabled,
            app_role: config.app_role.clone(),
            controller_service_enabled: config.controller_service_enabled,
            active_peer_device_id: config.active_peer_device_id.clone(),
        }),
        topology: Mutex::new(topology),
        clipboard: Mutex::new({
            let mut clipboard = ClipboardSyncEngine::new(identity.device_id);
            clipboard.set_enabled(config.clipboard_enabled);
            clipboard
        }),
        tuning: Mutex::new(InputTuningProfile {
            pointer_speed_multiplier: config.input_tuning.pointer_speed_multiplier,
            wheel_speed_multiplier: config.input_tuning.wheel_speed_multiplier,
            wheel_smoothing_factor: config.input_tuning.wheel_smoothing_factor,
        }),
        transfer_records: Mutex::new(Vec::new()),
        managed_devices: Mutex::new(managed_devices),
        tray_status: Mutex::new("foreground".into()),
        boot_error,
    })
}

async fn wait_until_ready() -> Result<CoreToUiEvent> {
    wait_until_ready_with_retry(20, 100).await
}

async fn wait_until_ready_with_retry(attempts: usize, delay_ms: u64) -> Result<CoreToUiEvent> {
    for _ in 0..attempts {
        match send_command(UiToCoreCommand::Ping).await {
            Ok(event) => return Ok(event),
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await,
        }
    }

    anyhow::bail!("timed out waiting for core-service readiness")
}

fn install_tray(app: &mut tauri::App) -> Result<()> {
    let show = MenuItem::with_id(app, "show_main", "打开 Deskflow-Plus", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide_main", "隐藏窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit_app", "退出", true, None::<&str>)?;
    let menu = MenuBuilder::new(app)
        .item(&show)
        .item(&hide)
        .separator()
        .item(&quit)
        .build()?;

    let mut tray = TrayIconBuilder::with_id("deskflow-plus-tray")
        .menu(&menu)
        .tooltip("Deskflow-Plus")
        .show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }
    tray.build(app)?;
    Ok(())
}

fn reveal_main_window(app: &tauri::AppHandle) -> Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        anyhow::bail!("main window not found");
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut tray_status) = state.tray_status.lock() {
            *tray_status = "foreground".into();
        }
    }
    Ok(())
}

fn handle_menu_event(app: &tauri::AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "show_main" => {
            let _ = reveal_main_window(app);
        }
        "hide_main" => {
            let Some(window) = app.get_webview_window("main") else {
                return;
            };
            let _ = window.hide();
            if let Some(state) = app.try_state::<AppState>() {
                if let Ok(mut tray_status) = state.tray_status.lock() {
                    *tray_status = "background".into();
                }
            }
        }
        "quit_app" => app.exit(0),
        _ => {}
    }
}

fn handle_tray_event(app: &tauri::AppHandle, event: TrayIconEvent) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        let visible = window.is_visible().unwrap_or(true);
        let next_status = if visible {
            let _ = window.hide();
            "background"
        } else {
            let _ = reveal_main_window(app);
            "foreground"
        };

        if let Some(state) = app.try_state::<AppState>() {
            if let Ok(mut tray_status) = state.tray_status.lock() {
                *tray_status = next_status.into();
            }
        }

        let _ = app.emit("tray-status", next_status);
    }
}

#[tauri::command]
fn get_app_health(state: tauri::State<'_, AppState>) -> Result<AppHealth, String> {
    state
        .health
        .lock()
        .map(|health| health.clone())
        .map_err(|_| "failed to access app health".to_string())
}

#[tauri::command]
fn get_runtime_overview(state: tauri::State<'_, AppState>) -> Result<RuntimeOverview, String> {
    let health = state
        .health
        .lock()
        .map_err(|_| "failed to access app health".to_string())?
        .clone();
    let clipboard_enabled = state
        .clipboard
        .lock()
        .map_err(|_| "failed to access clipboard state".to_string())?
        .enabled();
    let tuning = *state
        .tuning
        .lock()
        .map_err(|_| "failed to access input tuning".to_string())?;
    let tray_status = state
        .tray_status
        .lock()
        .map_err(|_| "failed to access tray status".to_string())?
        .clone();
    let boot_error = state
        .boot_error
        .lock()
        .map_err(|_| "failed to access boot error".to_string())?
        .clone();

    Ok(RuntimeOverview {
        health,
        input_status: current_platform_input_status(),
        cursor_sample: sample_cursor_position().ok(),
        clipboard_enabled,
        tuning,
        tray_status,
        boot_error,
    })
}

#[tauri::command]
fn get_connection_state(state: tauri::State<'_, AppState>) -> Result<ConnectionStateDto, String> {
    let config = load_or_create_config(&state.data_paths).map_err(|error| error.to_string())?;
    {
        let mut config_slot = state
            .config
            .lock()
            .map_err(|_| "failed to access app config".to_string())?;
        *config_slot = config.clone();
    }
    let trust_store = load_trust_store(&state.data_paths).map_err(|error| error.to_string())?;
    let discovery_peers =
        load_discovery_peers(&state.data_paths).map_err(|error| error.to_string())?;
    let managed = managed_devices_from_trust_store(
        &trust_store,
        unix_time_now_ms(),
        DEFAULT_OFFLINE_AFTER_MS,
    );
    {
        let mut managed_slot = state
            .managed_devices
            .lock()
            .map_err(|_| "failed to access managed devices".to_string())?;
        *managed_slot = managed.clone();
    }
    let now = unix_time_now_ms();
    let active_peer_state = if let Some(device_id) = config.active_peer_device_id.as_ref() {
        let trusted_match = trust_store.trusted_device(
            Uuid::parse_str(device_id).map_err(|error: uuid::Error| error.to_string())?,
        );
        let managed_match = managed
            .iter()
            .find(|device| device.device_id.to_string() == *device_id);
        let discovery_match = discovery_peers
            .iter()
            .find(|peer| peer.device_id == *device_id);
        let controller_peer_online = discovery_match.is_some_and(|peer| {
            now.saturating_sub(peer.discovered_at_unix_ms) <= DISCOVERY_PEER_TTL_MS
        });
        let managed_peer_online =
            managed_match.is_some_and(|device| device.status == ManagedDeviceStatus::Online);
        if config.app_role == "client" {
            if trusted_match.is_some() && controller_peer_online {
                "connected"
            } else if trusted_match.is_none() {
                "pending"
            } else {
                "disconnected"
            }
        } else if config.controller_service_enabled && managed_peer_online {
            "connected"
        } else if config.controller_service_enabled {
            "disconnected"
        } else {
            "disconnected"
        }
    } else {
        "disconnected"
    };
    let active_peer_display_name = config.active_peer_device_id.as_ref().and_then(|device_id| {
        managed
            .iter()
            .find(|device| device.device_id.to_string() == *device_id)
            .map(|device| device.display_name.clone())
            .or_else(|| {
                discovery_peers
                    .iter()
                    .find(|peer| peer.device_id == *device_id)
                    .map(|peer| peer.display_name.clone())
            })
    });
    let pending = load_pending_pairing_requests(&state.data_paths)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|request| PendingPairingRequestDto {
            device_id: request.device_id,
            display_name: request.display_name,
            platform: request.platform,
            address: request.address,
            port: request.port,
            pairing_code: request.pairing_code,
            received_at_unix_ms: request.received_at_unix_ms,
        })
        .collect();

    Ok(ConnectionStateDto {
        app_role: config.app_role,
        controller_service_enabled: config.controller_service_enabled,
        current_pairing_code: config.current_pairing_code,
        active_peer_device_id: config.active_peer_device_id,
        active_peer_display_name,
        active_peer_state: active_peer_state.into(),
        last_pairing_error: config.last_pairing_error,
        pending_pairing_requests: pending,
    })
}

#[tauri::command]
fn set_app_role(
    state: tauri::State<'_, AppState>,
    request: SetAppRoleRequest,
) -> Result<ConnectionStateDto, String> {
    let role = request.role.trim().to_lowercase();
    if role != "controller" && role != "client" {
        return Err("invalid app role".into());
    }

    {
        let health = state
            .health
            .lock()
            .map_err(|_| "failed to access app health".to_string())?;
        if health.controller_service_enabled && role == "client" {
            return Err("主控服务启用时不能切换为被控端".into());
        }
    }

    {
        let mut config = state
            .config
            .lock()
            .map_err(|_| "failed to access app config".to_string())?;
        config.app_role = role.clone();
        if role != "client" {
            config.last_pairing_error = None;
        }
        save_config(&state.data_paths, &config).map_err(|error| error.to_string())?;
    }

    {
        let mut health = state
            .health
            .lock()
            .map_err(|_| "failed to access app health".to_string())?;
        health.app_role = role;
    }

    get_connection_state(state)
}

#[tauri::command]
fn set_controller_service_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<ConnectionStateDto, String> {
    {
        let mut config = state
            .config
            .lock()
            .map_err(|_| "failed to access app config".to_string())?;
        config.app_role = "controller".into();
        config.controller_service_enabled = enabled;
        if enabled {
            if config.current_pairing_code.is_none() {
                config.current_pairing_code = Some(generate_pairing_code());
            }
            config.last_pairing_error = None;
        } else {
            config.current_pairing_code = None;
            config.active_peer_device_id = None;
            config.last_pairing_error = None;
        }
        save_config(&state.data_paths, &config).map_err(|error| error.to_string())?;
    }
    if !enabled {
        save_discovery_peers(&state.data_paths, &[]).map_err(|error| error.to_string())?;
        let identity = load_or_create_identity(&state.data_paths, &default_display_name())
            .map_err(|error| error.to_string())?;
        let frame =
            core_protocol::ProtocolFrame::new(core_protocol::ProtocolMessage::DiscoverWithdraw {
                device_id: identity.device_id.to_string(),
            })
            .encode_json_line()
            .map_err(|error| error.to_string())?;
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|error| error.to_string())?;
        let _ = socket.set_broadcast(true);
        let _ = socket.send_to(&frame, format!("255.255.255.255:{DISCOVERY_PORT}"));
    }

    {
        let mut health = state
            .health
            .lock()
            .map_err(|_| "failed to access app health".to_string())?;
        health.app_role = "controller".into();
        health.controller_service_enabled = enabled;
        if !enabled {
            health.active_peer_device_id = None;
        }
    }

    sync_topology_with_trusted_devices(&state)?;
    get_connection_state(state)
}

#[tauri::command]
fn submit_discovery_pairing_request(
    state: tauri::State<'_, AppState>,
    request: DiscoveryTrustRequest,
) -> Result<String, String> {
    let peer = load_discovery_peers(&state.data_paths)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|peer| peer.device_id == request.device_id)
        .ok_or_else(|| "未找到可发起请求的主控设备".to_string())?;

    let local_identity = load_or_create_identity(&state.data_paths, &default_display_name())
        .map_err(|error| error.to_string())?;
    if peer.device_id == local_identity.device_id.to_string() {
        return Err("不能和自己设备配对".into());
    }
    let local_certificate = load_or_create_certificate(&state.data_paths, &local_identity)
        .map_err(|error| error.to_string())?;
    let local_descriptor = session_descriptor(
        &local_identity,
        &local_certificate,
        &manual_endpoint(detect_local_host_ip(), SESSION_PORT),
    );
    let pairing_code = format!("auto:{}", unix_time_now_ms());
    let frame = core_protocol::ProtocolFrame::new(core_protocol::ProtocolMessage::PairRequest {
        device: local_descriptor,
        pairing_code: PairingCode {
            value: pairing_code.clone(),
        },
    })
    .encode_json_line()
    .map_err(|error| error.to_string())?;

    let socket = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|error| error.to_string())?;
    socket
        .send_to(&frame, format!("{}:{}", peer.address, DISCOVERY_PORT))
        .map_err(|error| error.to_string())?;

    {
        let mut config = state
            .config
            .lock()
            .map_err(|_| "failed to access app config".to_string())?;
        config.app_role = "client".into();
        config.active_peer_device_id = Some(peer.device_id.clone());
        config.last_pairing_error = None;
        save_config(&state.data_paths, &config).map_err(|error| error.to_string())?;
    }
    {
        let mut health = state
            .health
            .lock()
            .map_err(|_| "failed to access app health".to_string())?;
        health.app_role = "client".into();
        health.active_peer_device_id = Some(peer.device_id.clone());
    }

    Ok(pairing_code)
}

#[tauri::command]
fn respond_to_pending_pairing(
    state: tauri::State<'_, AppState>,
    request: ConfirmPendingPairingRequest,
) -> Result<ConnectionStateDto, String> {
    let controller_config =
        load_or_create_config(&state.data_paths).map_err(|error| error.to_string())?;
    if controller_config.app_role != "controller" {
        return Err("当前设备不是主控端，不能处理配对请求".into());
    }
    if !controller_config.controller_service_enabled {
        return Err("请先启用主控端服务，再处理连接请求".into());
    }

    let local_identity = load_or_create_identity(&state.data_paths, &default_display_name())
        .map_err(|error| error.to_string())?;
    let pending =
        load_pending_pairing_requests(&state.data_paths).map_err(|error| error.to_string())?;
    let target = pending
        .iter()
        .find(|item| item.device_id == request.device_id)
        .cloned()
        .ok_or_else(|| "未找到待确认配对请求".to_string())?;

    if target.device_id == local_identity.device_id.to_string() {
        return Err("不能和自己设备配对".into());
    }

    let remaining = pending
        .into_iter()
        .filter(|item| item.device_id != request.device_id)
        .collect::<Vec<_>>();
    save_pending_pairing_requests(&state.data_paths, &remaining)
        .map_err(|error| error.to_string())?;

    if request.accept {
        let is_auto_discovery_request = target.pairing_code.starts_with("auto:");
        if !is_auto_discovery_request {
            let expected_code = controller_config
                .current_pairing_code
                .clone()
                .ok_or_else(|| "主控端当前没有可用配对码".to_string())?;
            if target.pairing_code != expected_code {
                return Err("配对码校验失败，请刷新主控端配对码后重试".into());
            }
        }

        let pairing_request = PairingRequest {
            requester: DeviceDescriptor {
                device_id: target.device_id.clone(),
                display_name: target.display_name.clone(),
                platform: target.platform.clone(),
                address: target.address.clone(),
                port: target.port,
                fingerprint_sha256: target.fingerprint_sha256.clone(),
                certificate_pem: target.certificate_pem.clone(),
            },
            pairing_code: PairingCode {
                value: target.pairing_code.clone(),
            },
        };
        process_pairing_request(&state.data_paths, pairing_request, PairingDecision::Accept)
            .map_err(|error| error.to_string())?;
        upsert_cached_peer_descriptor(
            &state.data_paths,
            CachedPeerDescriptor {
                device_id: target.device_id.clone(),
                display_name: target.display_name.clone(),
                platform: target.platform.clone(),
                address: target.address.clone(),
                port: target.port,
                fingerprint_sha256: target.fingerprint_sha256.clone(),
                certificate_pem: target.certificate_pem.clone(),
                updated_at_unix_ms: unix_time_now_ms(),
            },
        )
        .map_err(|error| error.to_string())?;
        sync_topology_with_trusted_devices(&state)?;

        {
            let mut config = state
                .config
                .lock()
                .map_err(|_| "failed to access app config".to_string())?;
            config.active_peer_device_id = Some(target.device_id.clone());
            save_config(&state.data_paths, &config).map_err(|error| error.to_string())?;
        }
        {
            let mut health = state
                .health
                .lock()
                .map_err(|_| "failed to access app health".to_string())?;
            health.active_peer_device_id = Some(target.device_id.clone());
        }
        let frame = core_protocol::ProtocolFrame::new(core_protocol::ProtocolMessage::PairAccept {
            device_id: local_identity.device_id.to_string(),
        })
        .encode_json_line()
        .map_err(|error| error.to_string())?;
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|error| error.to_string())?;
        socket
            .send_to(&frame, format!("{}:{}", target.address, DISCOVERY_PORT))
            .map_err(|error| error.to_string())?;
    } else {
        let frame = core_protocol::ProtocolFrame::new(core_protocol::ProtocolMessage::PairReject {
            device_id: local_identity.device_id.to_string(),
            reason: "rejected".into(),
        })
        .encode_json_line()
        .map_err(|error| error.to_string())?;
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|error| error.to_string())?;
        socket
            .send_to(&frame, format!("{}:{}", target.address, DISCOVERY_PORT))
            .map_err(|error| error.to_string())?;
    }

    get_device_management_snapshot(state.clone())?;
    get_connection_state(state)
}

#[tauri::command]
fn disconnect_active_peer(state: tauri::State<'_, AppState>) -> Result<ConnectionStateDto, String> {
    {
        let mut config = state
            .config
            .lock()
            .map_err(|_| "failed to access app config".to_string())?;
        config.active_peer_device_id = None;
        config.last_pairing_error = None;
        if config.app_role == "controller" {
            config.controller_service_enabled = false;
            config.current_pairing_code = None;
        }
        save_config(&state.data_paths, &config).map_err(|error| error.to_string())?;
    }
    {
        let mut health = state
            .health
            .lock()
            .map_err(|_| "failed to access app health".to_string())?;
        health.active_peer_device_id = None;
        if health.app_role == "controller" {
            health.controller_service_enabled = false;
        }
    }
    get_connection_state(state)
}

#[tauri::command]
fn get_topology_snapshot(state: tauri::State<'_, AppState>) -> Result<TopologySnapshot, String> {
    let topology = state
        .topology
        .lock()
        .map_err(|_| "failed to access topology".to_string())?;
    Ok(snapshot_from_layout(&topology))
}

#[tauri::command]
fn add_pending_topology_device(
    state: tauri::State<'_, AppState>,
    request: PendingDeviceRequest,
) -> Result<TopologySnapshot, String> {
    let mut topology = state
        .topology
        .lock()
        .map_err(|_| "failed to access topology".to_string())?;
    let mut next = topology.clone();
    next.add_pending_device(Uuid::new_v4(), request.display_name)
        .map_err(|error| error.to_string())?;
    let update = apply_hot_update(&topology, next).map_err(|error| error.to_string())?;
    save_topology(&state.data_paths, &update.layout).map_err(|error| error.to_string())?;
    *topology = update.layout.clone();
    update_health_version(&state, topology.version)?;
    Ok(snapshot_from_layout(&topology))
}

#[tauri::command]
fn place_topology_device(
    state: tauri::State<'_, AppState>,
    device_id: String,
    position: GridPositionDto,
) -> Result<TopologySnapshot, String> {
    let device_id = Uuid::parse_str(&device_id).map_err(|error: uuid::Error| error.to_string())?;
    let mut topology = state
        .topology
        .lock()
        .map_err(|_| "failed to access topology".to_string())?;
    let mut next = topology.clone();
    next.place_device(
        device_id,
        GridPosition {
            x: position.x,
            y: position.y,
        },
    )
    .map_err(|error| error.to_string())?;
    let update = apply_hot_update(&topology, next).map_err(|error| error.to_string())?;
    save_topology(&state.data_paths, &update.layout).map_err(|error| error.to_string())?;
    *topology = update.layout.clone();
    update_health_version(&state, topology.version)?;
    Ok(snapshot_from_layout(&topology))
}

#[tauri::command]
fn mark_topology_device_offline(
    state: tauri::State<'_, AppState>,
    device_id: String,
) -> Result<TopologySnapshot, String> {
    let device_id = Uuid::parse_str(&device_id).map_err(|error: uuid::Error| error.to_string())?;
    let mut topology = state
        .topology
        .lock()
        .map_err(|_| "failed to access topology".to_string())?;
    let mut next = topology.clone();
    next.mark_offline(device_id)
        .map_err(|error| error.to_string())?;
    let update = apply_hot_update(&topology, next).map_err(|error| error.to_string())?;
    save_topology(&state.data_paths, &update.layout).map_err(|error| error.to_string())?;
    *topology = update.layout.clone();
    update_health_version(&state, topology.version)?;
    Ok(snapshot_from_layout(&topology))
}

#[tauri::command]
fn get_clipboard_state(state: tauri::State<'_, AppState>) -> Result<ClipboardStateDto, String> {
    let clipboard = state
        .clipboard
        .lock()
        .map_err(|_| "failed to access clipboard state".to_string())?;
    Ok(ClipboardStateDto {
        enabled: clipboard.enabled(),
        local_device_id: clipboard.local_device_id().to_string(),
    })
}

#[tauri::command]
fn get_device_profile(state: tauri::State<'_, AppState>) -> Result<DeviceProfileDto, String> {
    let identity = load_or_create_identity(&state.data_paths, &default_display_name())
        .map_err(|error| error.to_string())?;
    Ok(DeviceProfileDto {
        device_id: identity.device_id.to_string(),
        display_name: identity.display_name,
        platform: identity.platform,
        lan_ip: detect_local_host_ip(),
        session_port: SESSION_PORT,
    })
}

#[tauri::command]
fn update_device_profile(
    state: tauri::State<'_, AppState>,
    request: UpdateDeviceProfileRequest,
) -> Result<DeviceProfileDto, String> {
    let mut identity = load_or_create_identity(&state.data_paths, &default_display_name())
        .map_err(|error| error.to_string())?;
    let next_name = request.display_name.trim();
    if next_name.is_empty() {
        return Err("设备名称不能为空".into());
    }
    identity.display_name = next_name.to_string();
    save_identity(&state.data_paths, &identity).map_err(|error| error.to_string())?;

    let mut topology = state
        .topology
        .lock()
        .map_err(|_| "failed to access topology".to_string())?;
    if let Some(device) = topology
        .devices
        .iter_mut()
        .find(|device| device.device_id == identity.device_id)
    {
        device.display_name = identity.display_name.clone();
        save_topology(&state.data_paths, &topology).map_err(|error| error.to_string())?;
    }

    Ok(DeviceProfileDto {
        device_id: identity.device_id.to_string(),
        display_name: identity.display_name,
        platform: identity.platform,
        lan_ip: detect_local_host_ip(),
        session_port: SESSION_PORT,
    })
}

#[tauri::command]
fn set_clipboard_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<ClipboardStateDto, String> {
    let mut clipboard = state
        .clipboard
        .lock()
        .map_err(|_| "failed to access clipboard state".to_string())?;
    clipboard.set_enabled(enabled);

    {
        let mut health = state
            .health
            .lock()
            .map_err(|_| "failed to access app health".to_string())?;
        health.clipboard_enabled = enabled;
    }

    {
        let mut config = state
            .config
            .lock()
            .map_err(|_| "failed to access app config".to_string())?;
        config.clipboard_enabled = enabled;
        save_config(&state.data_paths, &config).map_err(|error| error.to_string())?;
    }

    Ok(ClipboardStateDto {
        enabled: clipboard.enabled(),
        local_device_id: clipboard.local_device_id().to_string(),
    })
}

#[tauri::command]
fn simulate_clipboard_broadcast(
    state: tauri::State<'_, AppState>,
    request: ClipboardTextRequest,
) -> Result<String, String> {
    let mut clipboard = state
        .clipboard
        .lock()
        .map_err(|_| "failed to access clipboard state".to_string())?;
    let update = clipboard
        .create_local_update(request.text)
        .ok_or_else(|| "clipboard broadcast suppressed".to_string())?;
    Ok(update.update.payload.text)
}

#[tauri::command]
fn simulate_image_clipboard_broadcast(
    state: tauri::State<'_, AppState>,
    request: ClipboardImageRequest,
) -> Result<ClipboardImageDto, String> {
    let mut clipboard = state
        .clipboard
        .lock()
        .map_err(|_| "failed to access clipboard state".to_string())?;
    let bytes = deterministic_bgra_image(request.width, request.height)
        .map_err(|error| error.to_string())?;
    let update = clipboard
        .create_local_image_update(
            ImageClipboardFormat::Bgra8,
            request.width,
            request.height,
            bytes,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "image clipboard broadcast suppressed".to_string())?;
    let core_clipboard::ClipboardContent::Image(payload) = update.content else {
        return Err("unexpected clipboard content type".into());
    };
    Ok(ClipboardImageDto {
        width: payload.width,
        height: payload.height,
        bytes_len: payload.bytes.len(),
        checksum_sha256: payload.checksum_sha256,
    })
}

#[tauri::command]
fn create_pairing_offer(state: tauri::State<'_, AppState>) -> Result<PairingOfferDto, String> {
    let identity = load_or_create_identity(&state.data_paths, &default_display_name())
        .map_err(|error| error.to_string())?;
    let endpoint_host = detect_local_host_ip();
    let pairing_code = {
        let mut config = state
            .config
            .lock()
            .map_err(|_| "failed to access app config".to_string())?;
        let next = config
            .current_pairing_code
            .clone()
            .unwrap_or_else(generate_pairing_code);
        config.current_pairing_code = Some(next.clone());
        save_config(&state.data_paths, &config).map_err(|error| error.to_string())?;
        next
    };

    Ok(PairingOfferDto {
        display_name: identity.display_name,
        endpoint_host,
        session_port: SESSION_PORT,
        pairing_code,
    })
}

#[tauri::command]
fn accept_pairing_payload(
    state: tauri::State<'_, AppState>,
    request: PairingImportRequest,
) -> Result<DeviceManagementSnapshot, String> {
    let pairing_request: PairingRequest =
        serde_json::from_str(&request.payload).map_err(|error| error.to_string())?;
    process_pairing_request(&state.data_paths, pairing_request, PairingDecision::Accept)
        .map_err(|error| error.to_string())?;
    sync_topology_with_trusted_devices(&state)?;
    get_device_management_snapshot(state)
}

#[tauri::command]
fn connect_to_manual_endpoint(
    state: tauri::State<'_, AppState>,
    request: ManualPairingConnectRequest,
) -> Result<PairingConnectResultDto, String> {
    let host = request.host.trim();
    if host.is_empty() {
        return Err("请输入主控端 IP 地址".into());
    }
    let pairing_code = request.pairing_code.trim();
    if pairing_code.is_empty() {
        return Err("请输入配对码".into());
    }
    if is_local_endpoint_host(host) {
        return Err("不能连接当前设备自身".into());
    }

    let controller_descriptor = build_manual_peer_descriptor(host, request.port, pairing_code);
    let discovery_snapshot =
        load_discovery_peers(&state.data_paths).map_err(|error| error.to_string())?;
    let known_controller = discovery_snapshot
        .iter()
        .find(|peer| peer.address == host && peer.port == request.port)
        .cloned();

    let local_identity = load_or_create_identity(&state.data_paths, &default_display_name())
        .map_err(|error| error.to_string())?;
    if let Some(peer) = known_controller.as_ref() {
        if peer.device_id == local_identity.device_id.to_string() {
            return Err("不能和自己设备配对".into());
        }
    }
    let local_certificate = load_or_create_certificate(&state.data_paths, &local_identity)
        .map_err(|error| error.to_string())?;
    let local_descriptor = session_descriptor(
        &local_identity,
        &local_certificate,
        &manual_endpoint(detect_local_host_ip(), SESSION_PORT),
    );
    let frame = core_protocol::ProtocolFrame::new(core_protocol::ProtocolMessage::PairRequest {
        device: local_descriptor,
        pairing_code: PairingCode {
            value: pairing_code.to_string(),
        },
    })
    .encode_json_line()
    .map_err(|error| error.to_string())?;
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|error| error.to_string())?;
    socket
        .send_to(&frame, format!("{host}:{DISCOVERY_PORT}"))
        .map_err(|error| error.to_string())?;

    if let Some(peer) = known_controller.as_ref() {
        let pairing_request = PairingRequest {
            requester: DeviceDescriptor {
                device_id: peer.device_id.clone(),
                display_name: peer.display_name.clone(),
                platform: peer.platform.clone(),
                address: peer.address.clone(),
                port: peer.port,
                fingerprint_sha256: peer.fingerprint_sha256.clone(),
                certificate_pem: peer.certificate_pem.clone(),
            },
            pairing_code: PairingCode {
                value: pairing_code.to_string(),
            },
        };
        process_pairing_request(&state.data_paths, pairing_request, PairingDecision::Accept)
            .map_err(|error| error.to_string())?;
        upsert_cached_peer_descriptor(
            &state.data_paths,
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
        )
        .map_err(|error| error.to_string())?;
    }

    {
        let mut config = state
            .config
            .lock()
            .map_err(|_| "failed to access app config".to_string())?;
        config.app_role = "client".into();
        if let Some(peer) = known_controller.as_ref() {
            config.active_peer_device_id = Some(peer.device_id.clone());
        }
        config.last_pairing_error = None;
        save_config(&state.data_paths, &config).map_err(|error| error.to_string())?;
    }

    {
        let mut health = state
            .health
            .lock()
            .map_err(|_| "failed to access app health".to_string())?;
        health.app_role = "client".into();
        if let Some(peer) = known_controller.as_ref() {
            health.active_peer_device_id = Some(peer.device_id.clone());
        }
    }

    sync_topology_with_trusted_devices(&state)?;
    let device_management = get_device_management_snapshot(state.clone())?;

    Ok(PairingConnectResultDto {
        imported_payload: serde_json::to_string_pretty(&PairingRequest {
            requester: controller_descriptor,
            pairing_code: PairingCode {
                value: pairing_code.to_string(),
            },
        })
        .map_err(|error| error.to_string())?,
        pairing_code: pairing_code.to_string(),
        endpoint_host: host.to_string(),
        session_port: request.port,
        device_management,
    })
}

#[tauri::command]
fn list_discovered_peers(state: tauri::State<'_, AppState>) -> Result<Vec<DiscoveryPeer>, String> {
    let now = unix_time_now_ms();
    let mut peers = load_discovery_peers(&state.data_paths)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|peer| now.saturating_sub(peer.discovered_at_unix_ms) <= DISCOVERY_PEER_TTL_MS)
        .collect::<Vec<_>>();
    peers.sort_by(|left, right| {
        right
            .discovered_at_unix_ms
            .cmp(&left.discovered_at_unix_ms)
            .then_with(|| left.display_name.cmp(&right.display_name))
    });
    Ok(peers)
}

#[tauri::command]
fn trust_discovered_peer(
    state: tauri::State<'_, AppState>,
    request: DiscoveryTrustRequest,
) -> Result<DeviceManagementSnapshot, String> {
    let peer = load_discovery_peers(&state.data_paths)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|peer| peer.device_id == request.device_id)
        .ok_or_else(|| "未找到指定的自动发现设备".to_string())?;
    let pairing_request = PairingRequest {
        requester: discovery_peer_to_descriptor(&peer),
        pairing_code: PairingCode {
            value: generate_pairing_code(),
        },
    };
    process_pairing_request(&state.data_paths, pairing_request, PairingDecision::Accept)
        .map_err(|error| error.to_string())?;
    upsert_cached_peer_descriptor(
        &state.data_paths,
        descriptor_to_cached_peer(&discovery_peer_to_descriptor(&peer)),
    )
    .map_err(|error| error.to_string())?;
    sync_topology_with_trusted_devices(&state)?;
    get_device_management_snapshot(state)
}

#[tauri::command]
fn get_input_tuning(state: tauri::State<'_, AppState>) -> Result<InputTuningDto, String> {
    let tuning = *state
        .tuning
        .lock()
        .map_err(|_| "failed to access input tuning".to_string())?;
    Ok(InputTuningDto {
        pointer_speed_multiplier: tuning.pointer_speed_multiplier,
        wheel_speed_multiplier: tuning.wheel_speed_multiplier,
        wheel_smoothing_factor: tuning.wheel_smoothing_factor,
    })
}

#[tauri::command]
fn update_input_tuning(
    state: tauri::State<'_, AppState>,
    request: InputTuningRequest,
) -> Result<InputTuningDto, String> {
    let next = InputTuningProfile {
        pointer_speed_multiplier: request.pointer_speed_multiplier.clamp(0.25, 3.0),
        wheel_speed_multiplier: request.wheel_speed_multiplier.clamp(0.25, 3.0),
        wheel_smoothing_factor: request.wheel_smoothing_factor.clamp(0.0, 0.95),
    };
    let mut tuning = state
        .tuning
        .lock()
        .map_err(|_| "failed to access input tuning".to_string())?;
    *tuning = next;

    {
        let mut config = state
            .config
            .lock()
            .map_err(|_| "failed to access app config".to_string())?;
        config.input_tuning.pointer_speed_multiplier = next.pointer_speed_multiplier;
        config.input_tuning.wheel_speed_multiplier = next.wheel_speed_multiplier;
        config.input_tuning.wheel_smoothing_factor = next.wheel_smoothing_factor;
        save_config(&state.data_paths, &config).map_err(|error| error.to_string())?;
    }

    Ok(InputTuningDto {
        pointer_speed_multiplier: tuning.pointer_speed_multiplier,
        wheel_speed_multiplier: tuning.wheel_speed_multiplier,
        wheel_smoothing_factor: tuning.wheel_smoothing_factor,
    })
}

#[tauri::command]
async fn create_transfer_plan(
    state: tauri::State<'_, AppState>,
    request: TransferPlanRequest,
) -> Result<TransferRecord, String> {
    let data_paths = state.data_paths.clone();
    let source_device_id = {
        let clipboard = state
            .clipboard
            .lock()
            .map_err(|_| "failed to access clipboard state".to_string())?;
        clipboard.local_device_id()
    };
    let record = tauri::async_runtime::spawn_blocking(move || {
        create_transfer_plan_blocking(data_paths, source_device_id, request)
    })
    .await
    .map_err(|error| format!("file transfer worker failed: {error}"))?
    .map_err(|error| error.to_string())?;

    state
        .transfer_records
        .lock()
        .map_err(|_| "failed to access transfer records".to_string())?
        .push(record.clone());
    Ok(record)
}

fn create_transfer_plan_blocking(
    data_paths: AppPaths,
    source_device_id: Uuid,
    request: TransferPlanRequest,
) -> Result<TransferRecord> {
    let target_device_id =
        Uuid::parse_str(&request.target_device_id).context("parse transfer target device id")?;
    let trust_store = load_trust_store(&data_paths)?;
    let trusted_target = trust_store
        .trusted_device(target_device_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("target device is not trusted"))?;
    let managed_devices = managed_devices_from_trust_store(
        &trust_store,
        unix_time_now_ms(),
        DEFAULT_OFFLINE_AFTER_MS,
    );
    let target_online = managed_devices
        .iter()
        .find(|device| device.device_id == target_device_id)
        .is_some_and(|device| device.status == ManagedDeviceStatus::Online);
    if !target_online {
        anyhow::bail!("target device is currently offline");
    }

    let now = unix_time_now_ms();
    let discovery_peers = load_discovery_peers(&data_paths)?;
    let cached_peers = load_cached_peer_descriptors(&data_paths)?;
    let remote = discovery_peers
        .iter()
        .filter(|peer| now.saturating_sub(peer.discovered_at_unix_ms) <= DISCOVERY_PEER_TTL_MS)
        .find(|peer| peer.device_id == trusted_target.device_id.to_string())
        .map(discovery_peer_to_descriptor)
        .or_else(|| {
            cached_peers
                .iter()
                .find(|peer| peer.device_id == trusted_target.device_id.to_string())
                .map(cached_peer_to_descriptor)
        })
        .ok_or_else(|| anyhow::anyhow!("target device is not currently reachable"))?;
    let outbound_files = request
        .files
        .into_iter()
        .map(|file| {
            let path = PathBuf::from(&file.path);
            let metadata = fs::metadata(&path)
                .with_context(|| format!("read transfer source metadata {}", path.display()))?;
            if !metadata.is_file() {
                anyhow::bail!("transfer source is not a file: {}", path.display());
            }
            if file.size_bytes != 0 && file.size_bytes != metadata.len() {
                append_log(
                    &data_paths,
                    &format!(
                        "transfer source size changed for {}: ui={} actual={}",
                        path.display(),
                        file.size_bytes,
                        metadata.len()
                    ),
                )?;
            }
            Ok(TransferArtifactFile {
                name: file.name,
                source_path: path,
                size_bytes: metadata.len(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if outbound_files.is_empty() {
        anyhow::bail!("no files selected for transfer");
    }

    let transfer_files = outbound_files
        .iter()
        .map(|file| TransferFileDescriptor {
            file_id: Uuid::new_v4(),
            name: file.name.clone(),
            size_bytes: file.size_bytes,
        })
        .collect();
    let plan = approve_transfer(
        plan_transfer(source_device_id, target_device_id, transfer_files, None)
            .context("create transfer plan")?,
    );
    let record = execute_remote_transfer(&data_paths, &remote, plan, &outbound_files)?;
    persist_outbound_transfer_artifacts(&data_paths, &record, &outbound_files)?;
    Ok(record)
}

fn persist_outbound_transfer_artifacts(
    paths: &AppPaths,
    record: &TransferRecord,
    _files: &[TransferArtifactFile],
) -> Result<()> {
    paths.ensure_layout()?;
    let transfer_dir = paths
        .transfers_dir()
        .join(record.plan.manifest.transfer_id.to_string());
    fs::create_dir_all(&transfer_dir).context("create transfer artifact directory")?;

    let summary_path = transfer_dir.join("transfer-record.json");
    let summary = serde_json::to_string_pretty(record).context("serialize transfer record")?;
    fs::write(&summary_path, summary).context("write transfer record summary")?;

    Ok(())
}

#[tauri::command]
fn list_transfer_plans(state: tauri::State<'_, AppState>) -> Result<Vec<TransferRecord>, String> {
    let records =
        load_transfer_records_from_disk(&state.data_paths).map_err(|error| error.to_string())?;
    {
        let mut slot = state
            .transfer_records
            .lock()
            .map_err(|_| "failed to access transfer records".to_string())?;
        *slot = records.clone();
    }
    Ok(records)
}

#[tauri::command]
fn get_transfer_artifact_path(
    state: tauri::State<'_, AppState>,
    request: TransferArtifactRequest,
) -> Result<String, String> {
    resolve_transfer_artifact_path(&state.data_paths, &request.transfer_id, &request.file_name)
        .map(|path| path.display().to_string())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn reveal_transfer_artifact_location(
    state: tauri::State<'_, AppState>,
    request: TransferArtifactRequest,
) -> Result<(), String> {
    let path =
        resolve_transfer_artifact_path(&state.data_paths, &request.transfer_id, &request.file_name)
            .map_err(|error| error.to_string())?;
    reveal_path_in_system(&path).map_err(|error| error.to_string())
}

#[tauri::command]
fn get_device_management_snapshot(
    state: tauri::State<'_, AppState>,
) -> Result<DeviceManagementSnapshot, String> {
    let trust_store = load_trust_store(&state.data_paths).map_err(|error| error.to_string())?;
    let devices = managed_devices_from_trust_store(
        &trust_store,
        unix_time_now_ms(),
        DEFAULT_OFFLINE_AFTER_MS,
    );
    {
        let mut slot = state
            .managed_devices
            .lock()
            .map_err(|_| "failed to access managed devices".to_string())?;
        *slot = devices.clone();
    }
    Ok(DeviceManagementSnapshot {
        devices,
        offline_after_ms: DEFAULT_OFFLINE_AFTER_MS,
    })
}

#[tauri::command]
fn repair_managed_device(
    state: tauri::State<'_, AppState>,
    request: DeviceRepairRequest,
) -> Result<DeviceManagementSnapshot, String> {
    let device_id =
        Uuid::parse_str(&request.device_id).map_err(|error: uuid::Error| error.to_string())?;
    let action = parse_device_repair_action(&request.action)?;
    if action == DeviceRepairAction::Revoke {
        revoke_trusted_device(&state.data_paths, device_id).map_err(|error| error.to_string())?;
        remove_cached_peer_descriptor(&state.data_paths, &request.device_id)
            .map_err(|error| error.to_string())?;
        remove_device_from_topology(&state, device_id)?;
        {
            let mut config = state
                .config
                .lock()
                .map_err(|_| "failed to access app config".to_string())?;
            if config.active_peer_device_id.as_deref() == Some(&request.device_id) {
                config.active_peer_device_id = None;
                config.last_pairing_error = None;
                save_config(&state.data_paths, &config).map_err(|error| error.to_string())?;
            }
        }
        {
            let mut health = state
                .health
                .lock()
                .map_err(|_| "failed to access app health".to_string())?;
            if health.active_peer_device_id.as_deref() == Some(&request.device_id) {
                health.active_peer_device_id = None;
            }
        }
    }

    let mut devices = state
        .managed_devices
        .lock()
        .map_err(|_| "failed to access managed devices".to_string())?;
    if action == DeviceRepairAction::Revoke {
        devices.retain(|device| device.device_id != device_id);
    } else if let Some(existing) = devices
        .iter_mut()
        .find(|device| device.device_id == device_id)
    {
        *existing = match action {
            DeviceRepairAction::RetryNow => {
                schedule_device_reconnect(existing, existing.reconnect_attempt.saturating_add(1))
            }
            _ => apply_device_repair(existing, action, unix_time_now_ms()),
        };
    } else {
        let trust_store = load_trust_store(&state.data_paths).map_err(|error| error.to_string())?;
        *devices = managed_devices_from_trust_store(
            &trust_store,
            unix_time_now_ms(),
            DEFAULT_OFFLINE_AFTER_MS,
        );
    }

    Ok(DeviceManagementSnapshot {
        devices: devices.clone(),
        offline_after_ms: DEFAULT_OFFLINE_AFTER_MS,
    })
}

#[tauri::command]
fn get_log_preview(state: tauri::State<'_, AppState>) -> Result<LogPreviewDto, String> {
    let lines = read_recent_log_lines(&state.data_paths, 100).map_err(|error| error.to_string())?;
    Ok(LogPreviewDto {
        log_path: state.data_paths.log_file().display().to_string(),
        lines,
    })
}

#[tauri::command]
fn export_diagnostics(state: tauri::State<'_, AppState>) -> Result<DiagnosticExportDto, String> {
    let config = load_or_create_config(&state.data_paths).map_err(|error| error.to_string())?;
    let health = state
        .health
        .lock()
        .map_err(|_| "failed to access app health".to_string())?
        .clone();
    let topology = state
        .topology
        .lock()
        .map_err(|_| "failed to access topology".to_string())?
        .clone();
    let transfer_count = state
        .transfer_records
        .lock()
        .map_err(|_| "failed to access transfer records".to_string())?
        .len();
    let managed_devices = state
        .managed_devices
        .lock()
        .map_err(|_| "failed to access managed devices".to_string())?
        .clone();
    let tuning = *state
        .tuning
        .lock()
        .map_err(|_| "failed to access input tuning".to_string())?;
    let tray_status = state
        .tray_status
        .lock()
        .map_err(|_| "failed to access tray status".to_string())?
        .clone();

    let metrics = vec![
        metric(
            "protocol_version",
            health.protocol_version.to_string(),
            "passed",
        ),
        metric(
            "topology_version",
            health.topology_version.to_string(),
            "passed",
        ),
        metric(
            "topology_devices",
            topology.devices.len().to_string(),
            "passed",
        ),
        metric("transfer_records", transfer_count.to_string(), "passed"),
        metric(
            "managed_devices",
            managed_devices.len().to_string(),
            "passed",
        ),
        metric(
            "offline_devices",
            managed_devices
                .iter()
                .filter(|device| format!("{:?}", device.status) == "Offline")
                .count()
                .to_string(),
            "passed",
        ),
        metric("tray_status", tray_status, "passed"),
        metric(
            "pointer_speed_multiplier",
            format!("{:.2}", tuning.pointer_speed_multiplier),
            "passed",
        ),
        metric(
            "wheel_smoothing_factor",
            format!("{:.2}", tuning.wheel_smoothing_factor),
            "passed",
        ),
    ];
    let path = export_extended_diagnostic_snapshot(&state.data_paths, &config, metrics.clone(), 50)
        .map_err(|error| error.to_string())?;
    Ok(DiagnosticExportDto {
        path: path.display().to_string(),
        metrics,
    })
}

fn execute_remote_transfer(
    paths: &AppPaths,
    remote: &DeviceDescriptor,
    plan: TransferPlan,
    files: &[TransferArtifactFile],
) -> Result<TransferRecord> {
    let local_identity = load_or_create_identity(paths, &default_display_name())?;
    let started = std::time::Instant::now();
    let mut record = TransferRecord {
        progress: TransferProgress {
            transfer_id: plan.manifest.transfer_id,
            transferred_bytes: 0,
            total_bytes: plan.manifest.total_bytes,
            chunk_index: 0,
            total_chunks: plan.total_chunks,
            status: core_file_transfer::TransferStatus::InProgress,
        },
        plan: plan.clone(),
        verified_files: 0,
        elapsed_ms: 0,
        created_at_unix_ms: unix_time_now_ms(),
        direction: "outbound".into(),
        peer_device_id: Some(remote.device_id.clone()),
        peer_display_name: Some(remote.display_name.clone()),
        delivery_state: "sending".into(),
        delivery_message: None,
        confirmed_at_unix_ms: None,
        error: None,
        artifacts: files
            .iter()
            .map(|file| TransferArtifactRef {
                file_name: file.name.clone(),
                path: file.source_path.clone(),
            })
            .collect(),
    };
    let header = TransferSessionHeader {
        record: record.clone(),
        source_device_id: local_identity.device_id.to_string(),
        source_display_name: local_identity.display_name,
    };

    let client_config = std::sync::Arc::new(build_client_tls_config(paths, remote)?);
    let stream = std::net::TcpStream::connect((remote.address.as_str(), remote.port))
        .with_context(|| {
            format!(
                "connect remote transfer session {}:{}",
                remote.address, remote.port
            )
        })?;
    let transfer_timeout = Some(std::time::Duration::from_secs(TRANSFER_IO_TIMEOUT_SECS));
    stream
        .set_read_timeout(transfer_timeout)
        .context("set transfer read timeout")?;
    stream
        .set_write_timeout(transfer_timeout)
        .context("set transfer write timeout")?;
    let server_name = rustls::pki_types::ServerName::try_from(remote.device_id.clone())
        .map_err(|_| anyhow::anyhow!("invalid remote server name"))?;
    let connection = rustls::ClientConnection::new(client_config, server_name)
        .context("create transfer client tls connection")?;
    let mut tls = rustls::StreamOwned::new(connection, stream);
    let session_raw = serde_json::to_vec(&header).context("serialize transfer session header")?;
    tls.write_all(&(session_raw.len() as u64).to_be_bytes())
        .context("write transfer session header length")?;
    tls.write_all(&session_raw)
        .context("write transfer session header payload")?;

    let mut writer = BufWriter::new(tls);
    for (descriptor, file) in plan.manifest.files.iter().zip(files.iter()) {
        let mut reader = BufReader::new(
            fs::File::open(&file.source_path)
                .with_context(|| format!("open transfer source {}", file.source_path.display()))?,
        );
        let chunk_capacity = usize::try_from(plan.chunk_size_bytes)
            .map_err(|_| anyhow::anyhow!("transfer chunk size exceeds platform capacity"))?;
        let mut buffer = vec![0u8; chunk_capacity];
        let mut offset = 0u64;
        let mut chunk_index = 0u64;
        loop {
            let bytes_read = reader
                .read(&mut buffer)
                .with_context(|| format!("read transfer source {}", file.source_path.display()))?;
            if bytes_read == 0 {
                break;
            }
            let chunk_bytes = &buffer[..bytes_read];
            let is_last_chunk = offset + bytes_read as u64 >= descriptor.size_bytes;
            let chunk_header = TransferChunkHeader {
                transfer_id: plan.manifest.transfer_id,
                file_id: descriptor.file_id,
                file_name: descriptor.name.clone(),
                chunk_index,
                offset,
                size_bytes: bytes_read as u64,
                checksum_sha256: checksum_sha256(chunk_bytes),
                is_last_chunk,
            };
            let chunk_header_raw =
                serde_json::to_vec(&chunk_header).context("serialize transfer chunk header")?;
            writer
                .write_all(&(chunk_header_raw.len() as u64).to_be_bytes())
                .context("write transfer chunk header length")?;
            writer
                .write_all(&chunk_header_raw)
                .context("write transfer chunk header payload")?;
            writer
                .write_all(chunk_bytes)
                .context("write transfer chunk payload")?;
            record.progress.transferred_bytes = (record.progress.transferred_bytes
                + bytes_read as u64)
                .min(plan.manifest.total_bytes);
            record.progress.chunk_index = chunk_index;
            record.progress.status =
                if record.progress.transferred_bytes >= plan.manifest.total_bytes {
                    core_file_transfer::TransferStatus::Completed
                } else {
                    core_file_transfer::TransferStatus::InProgress
                };
            offset += bytes_read as u64;
            chunk_index += 1;
        }
    }
    writer
        .write_all(&0u64.to_be_bytes())
        .context("write transfer end marker")?;
    writer.flush().context("flush transfer payload")?;
    let mut tls = writer
        .into_inner()
        .map_err(|error| anyhow::anyhow!("extract transfer tls stream: {error}"))?;
    let mut length_bytes = [0u8; 8];
    tls.read_exact(&mut length_bytes)
        .context("read transfer ack length")?;
    let ack_len = u64::from_be_bytes(length_bytes);
    if ack_len == 0 || ack_len > MAX_TRANSFER_ACK_BYTES {
        anyhow::bail!("invalid transfer ack length: {ack_len}");
    }
    let mut ack_bytes = vec![0u8; ack_len as usize];
    tls.read_exact(&mut ack_bytes)
        .context("read transfer ack payload")?;
    let ack: TransferDeliveryAck =
        serde_json::from_slice(&ack_bytes).context("parse transfer ack payload")?;
    record.elapsed_ms = started.elapsed().as_millis();
    if !ack.ok {
        record.delivery_state = "failed".into();
        record.delivery_message = Some(ack.message);
        record.error = record.delivery_message.clone();
        return Ok(record);
    }
    record.delivery_state = "delivered".into();
    record.delivery_message = Some(ack.message);
    record.peer_device_id = Some(ack.receiver_device_id);
    record.peer_display_name = Some(ack.receiver_display_name);
    record.confirmed_at_unix_ms = Some(ack.confirmed_at_unix_ms);
    record.verified_files = ack.verified_files;
    record.progress.transferred_bytes = ack.total_bytes;
    record.progress.total_bytes = ack.total_bytes;
    record.progress.chunk_index = record.progress.total_chunks.saturating_sub(1);
    record.progress.status = core_file_transfer::TransferStatus::Completed;

    Ok(record)
}

fn load_transfer_records_from_disk(paths: &AppPaths) -> Result<Vec<TransferRecord>> {
    paths.ensure_layout()?;
    let mut records = Vec::new();

    for entry in fs::read_dir(paths.transfers_dir()).context("read transfer artifacts directory")? {
        let entry = entry.context("read transfer artifact entry")?;
        let summary_path = entry.path().join("transfer-record.json");
        if !summary_path.exists() {
            continue;
        }

        let raw = fs::read_to_string(&summary_path).with_context(|| {
            format!(
                "read transfer record summary from {}",
                summary_path.display()
            )
        })?;
        match serde_json::from_str::<TransferRecord>(&raw) {
            Ok(record) => records.push(record),
            Err(error) => {
                let _ = append_log(
                    paths,
                    &format!(
                        "skip invalid transfer record {}: {error:#}",
                        summary_path.display()
                    ),
                );
            }
        }
    }

    records.sort_by_key(|record| record.created_at_unix_ms);
    Ok(records)
}

fn resolve_transfer_artifact_path(
    paths: &AppPaths,
    transfer_id: &str,
    file_name: &str,
) -> Result<std::path::PathBuf> {
    let transfer_id = transfer_id.trim();
    let file_name = file_name.trim();
    if transfer_id.is_empty() || file_name.is_empty() {
        anyhow::bail!("transfer artifact identity is incomplete");
    }

    let summary_path = paths
        .transfers_dir()
        .join(transfer_id)
        .join("transfer-record.json");
    if summary_path.exists() {
        let raw = fs::read_to_string(&summary_path).with_context(|| {
            format!(
                "read transfer record summary from {}",
                summary_path.display()
            )
        })?;
        if let Ok(record) = serde_json::from_str::<TransferRecord>(&raw) {
            if let Some(artifact) = record
                .artifacts
                .iter()
                .find(|artifact| artifact.file_name == file_name)
            {
                if artifact.path.exists() {
                    return Ok(artifact.path.clone());
                }
            }
        }
    }

    let candidate = paths.transfers_dir().join(transfer_id).join(file_name);
    if !candidate.exists() {
        anyhow::bail!("transfer artifact not found: {}", candidate.display());
    }

    Ok(candidate)
}

fn reveal_path_in_system(path: &std::path::Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .context("open explorer for transfer artifact")?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .context("reveal transfer artifact in Finder")?;
        return Ok(());
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let target = path.parent().unwrap_or(path);
        std::process::Command::new("xdg-open")
            .arg(target)
            .spawn()
            .context("open transfer artifact directory")?;
        return Ok(());
    }
}

fn deterministic_bgra_image(width: u32, height: u32) -> Result<Vec<u8>> {
    if width == 0 || height == 0 {
        anyhow::bail!("image dimensions must be non-zero");
    }
    let pixels = usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
        .ok_or_else(|| anyhow::anyhow!("image dimensions exceed platform capacity"))?;
    let mut bytes = Vec::with_capacity(pixels * 4);
    for index in 0..pixels {
        bytes.push((index % 251) as u8);
        bytes.push(((index * 3) % 251) as u8);
        bytes.push(((index * 7) % 251) as u8);
        bytes.push(255);
    }
    Ok(bytes)
}

fn parse_device_repair_action(action: &str) -> Result<DeviceRepairAction, String> {
    match action {
        "mark_online" => Ok(DeviceRepairAction::MarkOnline),
        "retry_now" => Ok(DeviceRepairAction::RetryNow),
        "revoke" => Ok(DeviceRepairAction::Revoke),
        other => Err(format!("unknown repair action: {other}")),
    }
}

fn metric(
    name: impl Into<String>,
    value: impl Into<String>,
    status: impl Into<String>,
) -> DiagnosticMetric {
    DiagnosticMetric {
        name: name.into(),
        value: value.into(),
        status: status.into(),
    }
}

fn default_transfer_direction() -> String {
    "outbound".into()
}

fn default_delivery_state() -> String {
    "pending".into()
}

fn discovery_peer_to_descriptor(peer: &DiscoveryPeer) -> DeviceDescriptor {
    DeviceDescriptor {
        device_id: peer.device_id.clone(),
        display_name: peer.display_name.clone(),
        platform: peer.platform.clone(),
        address: peer.address.clone(),
        port: peer.port,
        fingerprint_sha256: peer.fingerprint_sha256.clone(),
        certificate_pem: peer.certificate_pem.clone(),
    }
}

fn descriptor_to_cached_peer(descriptor: &DeviceDescriptor) -> CachedPeerDescriptor {
    CachedPeerDescriptor {
        device_id: descriptor.device_id.clone(),
        display_name: descriptor.display_name.clone(),
        platform: descriptor.platform.clone(),
        address: descriptor.address.clone(),
        port: descriptor.port,
        fingerprint_sha256: descriptor.fingerprint_sha256.clone(),
        certificate_pem: descriptor.certificate_pem.clone(),
        updated_at_unix_ms: unix_time_now_ms(),
    }
}

fn cached_peer_to_descriptor(peer: &CachedPeerDescriptor) -> DeviceDescriptor {
    DeviceDescriptor {
        device_id: peer.device_id.clone(),
        display_name: peer.display_name.clone(),
        platform: peer.platform.clone(),
        address: peer.address.clone(),
        port: peer.port,
        fingerprint_sha256: peer.fingerprint_sha256.clone(),
        certificate_pem: peer.certificate_pem.clone(),
    }
}

fn build_manual_peer_descriptor(host: &str, port: u16, pairing_code: &str) -> DeviceDescriptor {
    let normalized = pairing_code.replace('-', "").to_lowercase();
    let suffix = if normalized.is_empty() {
        "manual".to_string()
    } else {
        normalized
    };
    DeviceDescriptor {
        device_id: format!(
            "00000000-0000-0000-0000-{}",
            format!("{suffix:0>12}")[..12].to_string()
        ),
        display_name: format!("Remote-{host}"),
        platform: "remote".into(),
        address: host.to_string(),
        port,
        fingerprint_sha256: format!("manual-{suffix}"),
        certificate_pem: format!(
            "-----BEGIN CERTIFICATE-----\nmanual-{host}-{port}-{suffix}\n-----END CERTIFICATE-----"
        ),
    }
}

fn sync_topology_with_trusted_devices(state: &tauri::State<'_, AppState>) -> Result<(), String> {
    let trust_store = load_trust_store(&state.data_paths).map_err(|error| error.to_string())?;
    let mut topology = state
        .topology
        .lock()
        .map_err(|_| "failed to access topology".to_string())?;
    let trusted_ids = trust_store
        .devices
        .iter()
        .filter(|device| device.revoked_at_unix_ms.is_none())
        .map(|device| device.device_id)
        .collect::<std::collections::HashSet<_>>();

    let controller_device_id = topology.controller_device_id;
    topology.devices.retain(|device| {
        device.device_id == controller_device_id || trusted_ids.contains(&device.device_id)
    });

    for device in trust_store
        .devices
        .into_iter()
        .filter(|device| device.revoked_at_unix_ms.is_none())
    {
        if topology
            .devices
            .iter()
            .any(|entry| entry.device_id == device.device_id)
        {
            continue;
        }
        topology
            .add_pending_device(device.device_id, device.display_name)
            .map_err(|error| error.to_string())?;
    }

    auto_place_pending_topology_devices(&mut topology)?;
    save_topology(&state.data_paths, &topology).map_err(|error| error.to_string())?;
    update_health_version(state, topology.version)
}

fn auto_place_pending_topology_devices(layout: &mut TopologyLayout) -> Result<(), String> {
    let pending_ids = layout
        .devices
        .iter()
        .filter(|device| device.position.is_none())
        .map(|device| device.device_id)
        .collect::<Vec<_>>();

    for device_id in pending_ids {
        if layout
            .device(device_id)
            .and_then(|device| device.position)
            .is_some()
        {
            continue;
        }

        let mut placed = false;
        for y in 0..layout.grid_height {
            for x in 0..layout.grid_width {
                if layout.device_at(GridPosition { x, y }).is_some() {
                    continue;
                }

                let mut candidate = layout.clone();
                if candidate
                    .place_device(device_id, GridPosition { x, y })
                    .is_ok()
                    && candidate.validate().is_ok()
                {
                    *layout = candidate;
                    placed = true;
                    break;
                }
            }
            if placed {
                break;
            }
        }
    }

    Ok(())
}

fn remove_device_from_topology(
    state: &tauri::State<'_, AppState>,
    device_id: Uuid,
) -> Result<(), String> {
    let mut topology = state
        .topology
        .lock()
        .map_err(|_| "failed to access topology".to_string())?;
    topology
        .devices
        .retain(|device| device.device_id != device_id);
    save_topology(&state.data_paths, &topology).map_err(|error| error.to_string())?;
    update_health_version(state, topology.version)
}

fn is_local_endpoint_host(host: &str) -> bool {
    let normalized = host.trim().to_ascii_lowercase();
    if normalized == "127.0.0.1" || normalized == "localhost" || normalized == "::1" {
        return true;
    }

    normalized == detect_local_host_ip().to_ascii_lowercase()
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

fn generate_pairing_code() -> String {
    format!("{:06}", (unix_time_now_ms() % 1_000_000) as u64)
}

fn unix_time_now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis()
}

fn update_health_version(
    state: &tauri::State<'_, AppState>,
    topology_version: u64,
) -> Result<(), String> {
    let mut health = state
        .health
        .lock()
        .map_err(|_| "failed to access app health".to_string())?;
    health.topology_version = topology_version;
    Ok(())
}

fn snapshot_from_layout(layout: &TopologyLayout) -> TopologySnapshot {
    TopologySnapshot {
        version: layout.version,
        grid_width: layout.grid_width,
        grid_height: layout.grid_height,
        controller_device_id: layout.controller_device_id.to_string(),
        devices: layout
            .devices
            .iter()
            .map(|device| TopologyDeviceDto {
                device_id: device.device_id.to_string(),
                display_name: device.display_name.clone(),
                position: device.position.map(|position| GridPositionDto {
                    x: position.x,
                    y: position.y,
                }),
                status: format!("{:?}", device.status),
            })
            .collect(),
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        if self.owns_core_service {
            let _ = self
                .runtime
                .block_on(send_command(UiToCoreCommand::Shutdown));
            if let Ok(mut join) = self.core_join.lock() {
                if let Some(handle) = join.take() {
                    let _ = handle.join();
                }
            }
        }
    }
}
