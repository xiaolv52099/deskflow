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
  created_at_unix_ms: number;
  direction: string;
  peer_device_id?: string | null;
  peer_display_name?: string | null;
  delivery_state: string;
  delivery_message?: string | null;
  confirmed_at_unix_ms?: number | null;
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
  input_status?: {
    platform: string;
    capture_ready: boolean;
    injection_ready: boolean;
    cursor_query_ready: boolean;
    permission_state: string;
    note: string;
  };
  clipboard_enabled?: boolean;
  tuning?: InputTuningDto;
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

export interface PendingPairingRequestDto {
  device_id: string;
  display_name: string;
  platform: string;
  address: string;
  port: number;
  pairing_code: string;
  received_at_unix_ms: number;
}

export interface ConnectionStateDto {
  app_role: string;
  controller_service_enabled: boolean;
  current_pairing_code: string | null;
  active_peer_device_id: string | null;
  active_peer_display_name: string | null;
  active_peer_state: string;
  last_pairing_error: string | null;
  pending_pairing_requests: PendingPairingRequestDto[];
}

export interface LogPreviewDto {
  log_path: string;
  lines: string[];
}

export interface DiagnosticExportDto {
  path: string;
  metrics: {
    name: string;
    value: string;
    status: string;
  }[];
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

export async function submitDiscoveryPairingRequest(device_id: string) {
  return invoke<string>("submit_discovery_pairing_request", {
    request: { device_id },
  });
}

export async function getConnectionState() {
  return invoke<ConnectionStateDto>("get_connection_state");
}

export async function setAppRole(role: string) {
  return invoke<ConnectionStateDto>("set_app_role", {
    request: { role },
  });
}

export async function setControllerServiceEnabled(enabled: boolean) {
  return invoke<ConnectionStateDto>("set_controller_service_enabled", {
    enabled,
  });
}

export async function respondToPendingPairing(device_id: string, accept: boolean) {
  return invoke<ConnectionStateDto>("respond_to_pending_pairing", {
    request: { device_id, accept },
  });
}

export async function disconnectActivePeer() {
  return invoke<ConnectionStateDto>("disconnect_active_peer");
}

export async function getDeviceManagementSnapshot() {
  return invoke<DeviceManagementSnapshot>("get_device_management_snapshot");
}

export async function repairManagedDevice(device_id: string, action: "mark_online" | "retry_now" | "revoke") {
  return invoke<DeviceManagementSnapshot>("repair_managed_device", {
    request: { device_id, action },
  });
}

export async function createTransferPlan(target_device_id: string, files: { name: string; size_bytes: number; bytes: number[] }[]) {
  return invoke<TransferRecord>("create_transfer_plan", {
    request: { target_device_id, files },
  });
}

export async function listTransferPlans() {
  return invoke<TransferRecord[]>("list_transfer_plans");
}

export async function getTransferArtifactPath(transfer_id: string, file_name: string) {
  return invoke<string>("get_transfer_artifact_path", {
    request: { transfer_id, file_name },
  });
}

export async function revealTransferArtifactLocation(transfer_id: string, file_name: string) {
  return invoke<void>("reveal_transfer_artifact_location", {
    request: { transfer_id, file_name },
  });
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

export async function exportDiagnostics() {
  return invoke<DiagnosticExportDto>("export_diagnostics");
}
