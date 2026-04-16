#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{Context as AnyhowContext, Result};
use core_clipboard::{ClipboardSyncEngine, ImageClipboardFormat};
use core_file_transfer::{
    approve_transfer, chunk_manifest_files, plan_transfer, transfer_pipeline_latency,
    TransferFileDescriptor, TransferPlan, TransferProgress, TransferReceiver,
};
use core_input::{
    current_platform_input_status, sample_cursor_position, InputTuningProfile, PlatformInputStatus,
};
use core_protocol::{DeviceDescriptor, PairingCode};
use core_service::run_core_service;
use core_session::{
    apply_device_repair, managed_devices_from_trust_store, schedule_device_reconnect,
    manual_endpoint, process_pairing_request, session_descriptor, DeviceRepairAction,
    ManagedDevice, PairingDecision, PairingRequest, DISCOVERY_PORT, SESSION_PORT,
    DEFAULT_OFFLINE_AFTER_MS,
};
use core_topology::{apply_hot_update, load_or_create_topology, save_topology, GridPosition, TopologyLayout};
use device_trust::{
    default_display_name, load_or_create_certificate, load_or_create_identity, load_trust_store,
    revoke_trusted_device, save_identity,
};
use foundation::{
    append_log, export_extended_diagnostic_snapshot, load_discovery_peers, load_or_create_config, read_recent_log_lines, AppPaths,
    DiagnosticMetric, DiscoveryPeer, DATA_ROOT_ENV_VAR,
};
use local_ipc::{send_command, CoreToUiEvent, UiToCoreCommand};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
const SINGLE_INSTANCE_MUTEX_NAME: &str = "DeskflowPlus.SingleInstance";
const MAIN_WINDOW_TITLE: &str = "Deskflow-Plus";

struct AppState {
    runtime: Runtime,
    owns_core_service: bool,
    core_join: Mutex<Option<std::thread::JoinHandle<()>>>,
    data_paths: AppPaths,
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
    payload: String,
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
    size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct TransferRecord {
    plan: TransferPlan,
    progress: TransferProgress,
    verified_files: usize,
    elapsed_ms: u128,
    error: Option<String>,
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
            create_transfer_plan,
            list_transfer_plans,
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
    let identity = load_or_create_identity(&paths, &default_display_name())?;
    let topology = load_or_create_topology(&paths, identity.device_id, &identity.display_name)?;
    let trust_store = load_trust_store(&paths)?;
    let managed_devices = managed_devices_from_trust_store(
        &trust_store,
        unix_time_now_ms(),
        DEFAULT_OFFLINE_AFTER_MS,
    );
    let boot_error = Arc::new(Mutex::new(None));
    let (owns_core_service, join) = match runtime.block_on(wait_until_ready()) {
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
    let protocol_version = match runtime.block_on(wait_until_ready()) {
        Ok(CoreToUiEvent::Ready { protocol_version, .. }) => protocol_version,
        Ok(other) => {
            if let Ok(mut slot) = boot_error.lock() {
                *slot = Some(format!("unexpected readiness event: {other:?}"));
            }
            core_protocol::CURRENT_PROTOCOL_VERSION
        }
        Err(error) => {
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
        health: Mutex::new(AppHealth {
            protocol_version,
            discovery_port: DISCOVERY_PORT,
            session_port: SESSION_PORT,
            topology_version: topology.version,
            clipboard_enabled: true,
            auto_discovery_enabled: true,
        }),
        topology: Mutex::new(topology),
        clipboard: Mutex::new(ClipboardSyncEngine::new(identity.device_id)),
        tuning: Mutex::new(InputTuningProfile::default()),
        transfer_records: Mutex::new(Vec::new()),
        managed_devices: Mutex::new(managed_devices),
        tray_status: Mutex::new("foreground".into()),
        boot_error,
    })
}

async fn wait_until_ready() -> Result<CoreToUiEvent> {
    for _ in 0..20 {
        match send_command(UiToCoreCommand::Ping).await {
            Ok(event) => return Ok(event),
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
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
    let device_id =
        Uuid::parse_str(&device_id).map_err(|error: uuid::Error| error.to_string())?;
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
    let device_id =
        Uuid::parse_str(&device_id).map_err(|error: uuid::Error| error.to_string())?;
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
        .create_local_image_update(ImageClipboardFormat::Bgra8, request.width, request.height, bytes)
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
    let certificate = load_or_create_certificate(&state.data_paths, &identity)
        .map_err(|error| error.to_string())?;
    let endpoint_host = detect_local_host_ip();
    let descriptor = session_descriptor(
        &identity,
        &certificate,
        &manual_endpoint(endpoint_host.clone(), SESSION_PORT),
    );
    let pairing_code = generate_pairing_code();
    let payload = serde_json::to_string_pretty(&PairingRequest {
        requester: descriptor,
        pairing_code: core_protocol::PairingCode {
            value: pairing_code.clone(),
        },
    })
    .map_err(|error| error.to_string())?;

    Ok(PairingOfferDto {
        display_name: identity.display_name,
        endpoint_host,
        session_port: SESSION_PORT,
        pairing_code,
        payload,
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
        return Err("主控端 IP 不能为空".into());
    }
    let pairing_code = request.pairing_code.trim();
    if pairing_code.is_empty() {
        return Err("配对码不能为空".into());
    }

    let payload = serde_json::to_string_pretty(&PairingRequest {
        requester: build_manual_peer_descriptor(host, request.port, pairing_code),
        pairing_code: PairingCode {
            value: pairing_code.to_string(),
        },
    })
    .map_err(|error| error.to_string())?;
    let device_management = accept_pairing_payload(
        state,
        PairingImportRequest {
            payload: payload.clone(),
        },
    )?;

    Ok(PairingConnectResultDto {
        imported_payload: payload,
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
    let mut tuning = state
        .tuning
        .lock()
        .map_err(|_| "failed to access input tuning".to_string())?;
    *tuning = InputTuningProfile {
        pointer_speed_multiplier: request.pointer_speed_multiplier.clamp(0.25, 3.0),
        wheel_speed_multiplier: request.wheel_speed_multiplier.clamp(0.25, 3.0),
        wheel_smoothing_factor: request.wheel_smoothing_factor.clamp(0.0, 0.95),
    };

    Ok(InputTuningDto {
        pointer_speed_multiplier: tuning.pointer_speed_multiplier,
        wheel_speed_multiplier: tuning.wheel_speed_multiplier,
        wheel_smoothing_factor: tuning.wheel_smoothing_factor,
    })
}

#[tauri::command]
fn create_transfer_plan(
    state: tauri::State<'_, AppState>,
    request: TransferPlanRequest,
) -> Result<TransferRecord, String> {
    let target_device_id =
        Uuid::parse_str(&request.target_device_id).map_err(|error: uuid::Error| error.to_string())?;
    let source_device_id = {
        let clipboard = state
            .clipboard
            .lock()
            .map_err(|_| "failed to access clipboard state".to_string())?;
        clipboard.local_device_id()
    };
    let files = request
        .files
        .into_iter()
        .map(|file| TransferFileDescriptor {
            file_id: Uuid::new_v4(),
            name: file.name,
            size_bytes: file.size_bytes,
        })
        .collect();
    let plan = approve_transfer(
        plan_transfer(source_device_id, target_device_id, files, None)
            .map_err(|error| error.to_string())?,
    );
    let record = execute_memory_transfer(plan).map_err(|error| error.to_string())?;
    state
        .transfer_records
        .lock()
        .map_err(|_| "failed to access transfer records".to_string())?
        .push(record.clone());
    Ok(record)
}

#[tauri::command]
fn list_transfer_plans(state: tauri::State<'_, AppState>) -> Result<Vec<TransferRecord>, String> {
    state
        .transfer_records
        .lock()
        .map(|records| records.clone())
        .map_err(|_| "failed to access transfer records".to_string())
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
    }

    let mut devices = state
        .managed_devices
        .lock()
        .map_err(|_| "failed to access managed devices".to_string())?;
    if let Some(existing) = devices.iter_mut().find(|device| device.device_id == device_id) {
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
        metric("protocol_version", health.protocol_version.to_string(), "passed"),
        metric("topology_version", health.topology_version.to_string(), "passed"),
        metric("topology_devices", topology.devices.len().to_string(), "passed"),
        metric("transfer_records", transfer_count.to_string(), "passed"),
        metric("managed_devices", managed_devices.len().to_string(), "passed"),
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

fn execute_memory_transfer(plan: TransferPlan) -> Result<TransferRecord> {
    let (completed, elapsed) = transfer_pipeline_latency(|| {
        let mut file_bytes = HashMap::new();
        for descriptor in &plan.manifest.files {
            file_bytes.insert(
                descriptor.file_id,
                deterministic_transfer_bytes(descriptor.file_id, descriptor.size_bytes)?,
            );
        }
        let chunks = chunk_manifest_files(&plan, &file_bytes)?;
        let mut receiver = TransferReceiver::new(plan.clone())?;
        let mut progress = receiver.progress(0);
        for chunk in chunks {
            progress = receiver.accept_chunk(chunk)?;
        }
        let completed = receiver.complete()?;
        anyhow::ensure!(
            completed
                .files
                .iter()
                .all(|file| file.bytes.len() as u64 == file.descriptor.size_bytes),
            "completed transfer failed size verification"
        );
        Ok::<_, anyhow::Error>((completed, progress))
    })?;

    Ok(TransferRecord {
        plan,
        progress: completed.1,
        verified_files: completed.0.files.len(),
        elapsed_ms: elapsed.as_millis(),
        error: None,
    })
}

fn deterministic_transfer_bytes(file_id: Uuid, size_bytes: u64) -> Result<Vec<u8>> {
    let size = usize::try_from(size_bytes)
        .map_err(|_| anyhow::anyhow!("file {file_id} exceeds platform capacity"))?;
    let seed = file_id.as_bytes();
    Ok((0..size)
        .map(|index| seed[index % seed.len()] ^ ((index % 251) as u8))
        .collect())
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

fn metric(name: impl Into<String>, value: impl Into<String>, status: impl Into<String>) -> DiagnosticMetric {
    DiagnosticMetric {
        name: name.into(),
        value: value.into(),
        status: status.into(),
    }
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

fn build_manual_peer_descriptor(host: &str, port: u16, pairing_code: &str) -> DeviceDescriptor {
    let normalized = pairing_code.replace('-', "").to_lowercase();
    let suffix = if normalized.is_empty() {
        "manual".to_string()
    } else {
        normalized
    };
    DeviceDescriptor {
        device_id: format!("00000000-0000-0000-0000-{}", format!("{suffix:0>12}")[..12].to_string()),
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

    for device in trust_store.devices.into_iter().filter(|device| device.revoked_at_unix_ms.is_none()) {
        if topology.devices.iter().any(|entry| entry.device_id == device.device_id) {
            continue;
        }
        topology
            .add_pending_device(device.device_id, device.display_name)
            .map_err(|error| error.to_string())?;
    }

    save_topology(&state.data_paths, &topology).map_err(|error| error.to_string())?;
    update_health_version(state, topology.version)
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

fn update_health_version(state: &tauri::State<'_, AppState>, topology_version: u64) -> Result<(), String> {
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
            let _ = self.runtime.block_on(send_command(UiToCoreCommand::Shutdown));
            if let Ok(mut join) = self.core_join.lock() {
                if let Some(handle) = join.take() {
                    let _ = handle.join();
                }
            }
        }
    }
}
