import { invoke } from "@tauri-apps/api/core";

export interface GridPositionDto {
  x: number;
  y: number;
}

export interface TopologyDeviceDto {
  device_id: string;
  display_name: string;
  position: GridPositionDto | null;
  status: string;
}

export interface TopologySnapshot {
  version: number;
  grid_width: number;
  grid_height: number;
  controller_device_id: string;
  devices: TopologyDeviceDto[];
}

export interface DeviceProfileDto {
  device_id: string;
  display_name: string;
  platform: string;
  lan_ip: string;
  session_port: number;
}

export interface PairingOfferDto {
  display_name: string;
  endpoint_host: string;
  session_port: number;
  pairing_code: string;
  payload: string;
}

export interface DiscoveryPeer {
  device_id: string;
  display_name: string;
  platform: string;
  address: string;
  port: number;
  fingerprint_sha256: string;
  certificate_pem: string;
  discovered_at_unix_ms: number;
}

export interface ManagedDevice {
  device_id: string;
  display_name: string;
  platform: string;
  status: string;
  last_seen_unix_ms: number | null;
  reconnect_attempt: number;
  next_retry_after_ms: number;
}

export interface DeviceManagementSnapshot {
  devices: ManagedDevice[];
  offline_after_ms: number;
}

export interface TransferRecord {
  plan: {
    manifest: {
      transfer_id: string;
      files: { file_id: string; name: string; size_bytes: number }[];
      total_bytes: number;
    };
    total_chunks: number;
  };
  progress: {
    status: string;
    transferred_bytes: number;
    total_bytes: number;
  };
  verified_files: number;
  elapsed_ms: number;
  error?: string | null;
}

export interface InputTuningDto {
  pointer_speed_multiplier: number;
  wheel_speed_multiplier: number;
  wheel_smoothing_factor: number;
}

export interface ClipboardStateDto {
  enabled: boolean;
  local_device_id: string;
}

export interface RuntimeOverview {
  health: {
    protocol_version: number;
    discovery_port: number;
    session_port: number;
    topology_version: number;
    clipboard_enabled: boolean;
    auto_discovery_enabled: boolean;
  };
  tray_status: string;
  boot_error?: string | null;
}

export interface PairingConnectResultDto {
  imported_payload: string;
  pairing_code: string;
  endpoint_host: string;
  session_port: number;
  device_management: DeviceManagementSnapshot;
}

export interface LogPreviewDto {
  log_path: string;
  lines: string[];
}

export async function getDeviceProfile() {
  return invoke<DeviceProfileDto>("get_device_profile");
}

export async function updateDeviceProfile(display_name: string) {
  return invoke<DeviceProfileDto>("update_device_profile", {
    request: { display_name },
  });
}

export async function getTopologySnapshot() {
  return invoke<TopologySnapshot>("get_topology_snapshot");
}

export async function placeTopologyDevice(device_id: string, position: GridPositionDto) {
  return invoke<TopologySnapshot>("place_topology_device", {
    device_id,
    position,
  });
}

export async function createPairingOffer() {
  return invoke<PairingOfferDto>("create_pairing_offer");
}

export async function connectToManualEndpoint(host: string, port: number, pairing_code: string) {
  return invoke<PairingConnectResultDto>("connect_to_manual_endpoint", {
    request: { host, port, pairing_code },
  });
}

export async function listDiscoveredPeers() {
  return invoke<DiscoveryPeer[]>("list_discovered_peers");
}

export async function trustDiscoveredPeer(device_id: string) {
  return invoke<DeviceManagementSnapshot>("trust_discovered_peer", {
    request: { device_id },
  });
}

export async function getDeviceManagementSnapshot() {
  return invoke<DeviceManagementSnapshot>("get_device_management_snapshot");
}

export async function createTransferPlan(target_device_id: string, files: { name: string; size_bytes: number }[]) {
  return invoke<TransferRecord>("create_transfer_plan", {
    request: { target_device_id, files },
  });
}

export async function listTransferPlans() {
  return invoke<TransferRecord[]>("list_transfer_plans");
}

export async function getInputTuning() {
  return invoke<InputTuningDto>("get_input_tuning");
}

export async function updateInputTuning(request: InputTuningDto) {
  return invoke<InputTuningDto>("update_input_tuning", { request });
}

export async function getClipboardState() {
  return invoke<ClipboardStateDto>("get_clipboard_state");
}

export async function setClipboardEnabled(enabled: boolean) {
  return invoke<ClipboardStateDto>("set_clipboard_enabled", { enabled });
}

export async function getRuntimeOverview() {
  return invoke<RuntimeOverview>("get_runtime_overview");
}

export async function getLogPreview() {
  return invoke<LogPreviewDto>("get_log_preview");
}
