import { useEffect, useMemo, useState } from "react";
import { useDrag, useDrop } from "react-dnd";
import { Check, CheckCircle2, Copy, Edit2, Monitor, MonitorSmartphone, PauseCircle, PlayCircle, RefreshCw, Server, Unplug } from "lucide-react";
import {
  connectToManualEndpoint,
  createPairingOffer,
  disconnectActivePeer,
  getConnectionState,
  getDeviceManagementSnapshot,
  getDeviceProfile,
  getTopologySnapshot,
  listDiscoveredPeers,
  placeTopologyDevice,
  repairManagedDevice,
  respondToPendingPairing,
  setAppRole,
  setControllerServiceEnabled,
  submitDiscoveryPairingRequest,
  updateDeviceProfile,
  type ConnectionStateDto,
  type DeviceManagementSnapshot,
  type DeviceProfileDto,
  type DiscoveryPeer,
  type TopologySnapshot,
} from "../lib/tauri";

const ItemType = "SCREEN";

interface ScreenBoxProps {
  id: string;
  name: string;
  isMaster: boolean;
  position: number;
}

function formatError(error: unknown) {
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return String(error);
}

function deviceStatusLabel(status: string) {
  const normalized = status.toLowerCase();
  if (normalized === "online") return "在线";
  if (normalized === "offline") return "离线";
  if (normalized === "revoked") return "已撤销";
  if (normalized === "reconnecting") return "重连中";
  if (normalized === "pending") return "待确认";
  return status;
}

function ScreenBox({ id, name, isMaster, position }: ScreenBoxProps) {
  const [{ isDragging }, drag] = useDrag(() => ({
    type: ItemType,
    item: { id, position },
    collect: (monitor) => ({ isDragging: monitor.isDragging() }),
  }));

  return (
    <div
      ref={(node) => drag(node)}
      className={`flex h-full w-full cursor-grab select-none flex-col items-center justify-center rounded-lg border p-1 shadow-sm transition-all active:cursor-grabbing ${
        isMaster ? "border-blue-500 bg-blue-900/60 text-blue-100 shadow-blue-900/20" : "border-slate-600 bg-slate-800 text-slate-200 hover:border-slate-500"
      } ${isDragging ? "scale-95 opacity-40" : "opacity-100 hover:scale-[1.02]"}`}
    >
      {isMaster ? <Monitor className="mb-1 text-blue-400" size={20} /> : <MonitorSmartphone className="mb-1 text-slate-400" size={16} />}
      <span className="w-full truncate text-center text-[10px] font-medium tracking-wide">{name}</span>
    </div>
  );
}

function GridCell({
  position,
  screen,
  onMoveScreen,
}: {
  position: number;
  screen?: ScreenBoxProps;
  onMoveScreen: (id: string, position: number) => void;
}) {
  const [{ isOver }, drop] = useDrop(() => ({
    accept: ItemType,
    drop: (item: { id: string }) => onMoveScreen(item.id, position),
    collect: (monitor) => ({ isOver: monitor.isOver() }),
  }));

  return (
    <div
      ref={(node) => drop(node)}
      className={`flex h-[68px] w-[68px] items-center justify-center rounded-xl border border-dashed p-1 transition-all ${
        isOver ? "scale-105 border-solid border-blue-400 bg-blue-500/10 shadow-inner" : "border-slate-700/60 bg-slate-900/40 hover:bg-slate-800/60"
      }`}
    >
      {screen ? <ScreenBox {...screen} /> : <span className="font-mono text-sm font-bold text-slate-700 opacity-30">{position + 1}</span>}
    </div>
  );
}

function clientCard(state: ConnectionStateDto | null) {
  if (state?.last_pairing_error) {
    return {
      title: "连接请求被拒绝",
      detail: state.last_pairing_error,
      className: "border-rose-500/20 bg-rose-500/10 text-rose-300",
    };
  }
  if (state?.active_peer_state === "connected" && state.active_peer_display_name) {
    return {
      title: "已连接主控端",
      detail: state.active_peer_display_name,
      className: "border-emerald-500/20 bg-emerald-400/10 text-emerald-400",
    };
  }
  if (state?.active_peer_state === "pending") {
    return {
      title: "等待主控端确认",
      detail: state.active_peer_display_name ? `已向 ${state.active_peer_display_name} 发起连接请求` : "已发起连接请求，等待主控端确认",
      className: "border-amber-500/20 bg-amber-400/10 text-amber-300",
    };
  }
  return {
    title: "当前未连接",
    detail: "可通过自动发现或手动配对连接主控端",
    className: "border-slate-700/60 bg-slate-900/50 text-slate-400",
  };
}

export function ConnectionConfig() {
  const [copied, setCopied] = useState<string | null>(null);
  const [isEditingName, setIsEditingName] = useState(false);
  const [machineName, setMachineName] = useState("Deskflow-Plus");
  const [profile, setProfile] = useState<DeviceProfileDto | null>(null);
  const [pairingCode, setPairingCode] = useState("");
  const [host, setHost] = useState("");
  const [hasInitializedHost, setHasInitializedHost] = useState(false);
  const [port, setPort] = useState("24801");
  const [manualPairingCode, setManualPairingCode] = useState("");
  const [topology, setTopology] = useState<TopologySnapshot | null>(null);
  const [deviceManagement, setDeviceManagement] = useState<DeviceManagementSnapshot | null>(null);
  const [discoveredPeers, setDiscoveredPeers] = useState<DiscoveryPeer[]>([]);
  const [connectionState, setConnectionState] = useState<ConnectionStateDto | null>(null);
  const [statusText, setStatusText] = useState("正在初始化连接配置...");
  const [error, setError] = useState("");
  const [discoveryPromptPeerId, setDiscoveryPromptPeerId] = useState<string | null>(null);
  const [controllerPromptDeviceId, setControllerPromptDeviceId] = useState<string | null>(null);
  const [dismissedDiscoveryPeerIds, setDismissedDiscoveryPeerIds] = useState<string[]>([]);

  const isController = connectionState?.app_role !== "client";

  async function refresh() {
    const [nextProfile, nextTopology, nextDevices, nextPeers, nextOffer, nextState] = await Promise.all([
      getDeviceProfile(),
      getTopologySnapshot(),
      getDeviceManagementSnapshot(),
      listDiscoveredPeers(),
      createPairingOffer(),
      getConnectionState(),
    ]);

    setProfile(nextProfile);
    if (!isEditingName) setMachineName(nextProfile.display_name);
    if (!hasInitializedHost && !host.trim()) {
      setHost(nextProfile.lan_ip);
      setHasInitializedHost(true);
    }
    setPairingCode(nextOffer.pairing_code);
    setTopology(nextTopology);
    setDeviceManagement(nextDevices);
    setDiscoveredPeers(nextPeers);
    setConnectionState(nextState);

    if (error) return;
    if (nextState.controller_service_enabled) {
      setStatusText(`主控端服务已启用，正在监听 ${nextProfile.lan_ip}:${nextProfile.session_port}`);
    } else if (nextState.last_pairing_error) {
      setStatusText(nextState.last_pairing_error);
    } else if (nextState.active_peer_state === "connected" && nextState.active_peer_display_name) {
      setStatusText(`当前已连接主控端：${nextState.active_peer_display_name}`);
    } else if (nextState.active_peer_state === "pending") {
      setStatusText(nextState.active_peer_display_name ? `已向 ${nextState.active_peer_display_name} 发起连接请求，等待主控端确认` : "已发起连接请求，等待主控端确认");
    } else {
      setStatusText("连接配置已加载");
    }
  }

  useEffect(() => {
    void refresh().catch((nextError) => setError(formatError(nextError)));
    const timer = window.setInterval(() => {
      void refresh().catch(() => undefined);
    }, 2500);
    return () => window.clearInterval(timer);
  }, [hasInitializedHost, host, isEditingName]);

  const screens = useMemo(() => {
    if (!topology) return [];
    return topology.devices
      .filter((device) => device.position)
      .map((device) => ({
        id: device.device_id,
        name: device.display_name,
        isMaster: device.device_id === topology.controller_device_id,
        position: device.position!.y * topology.grid_width + device.position!.x,
      }));
  }, [topology]);

  const availableDiscoveredPeers = useMemo(() => discoveredPeers.filter((peer) => peer.device_id !== profile?.device_id), [discoveredPeers, profile?.device_id]);
  const trustedDeviceIds = useMemo(() => new Set(deviceManagement?.devices.map((device) => device.device_id) ?? []), [deviceManagement?.devices]);
  const controllerServiceEnabled = Boolean(connectionState?.controller_service_enabled);
  const activePeerId = connectionState?.active_peer_device_id ?? null;
  const discoveryPromptPeer = availableDiscoveredPeers.find((peer) => peer.device_id === discoveryPromptPeerId) ?? null;
  const controllerPrompt = connectionState?.pending_pairing_requests.find((request) => request.device_id === controllerPromptDeviceId) ?? null;
  const currentClientCard = clientCard(connectionState);
  const hasClientSession = connectionState?.active_peer_state === "pending" || connectionState?.active_peer_state === "connected";
  const isActivePeerConnected = connectionState?.active_peer_state === "connected";

  useEffect(() => {
    if (discoveryPromptPeerId && !availableDiscoveredPeers.some((peer) => peer.device_id === discoveryPromptPeerId)) {
      setDiscoveryPromptPeerId(null);
    }
  }, [availableDiscoveredPeers, discoveryPromptPeerId]);

  useEffect(() => {
    if (!isController && !discoveryPromptPeerId && !hasClientSession) {
      const nextPeer = availableDiscoveredPeers.find(
        (peer) => !trustedDeviceIds.has(peer.device_id) && !dismissedDiscoveryPeerIds.includes(peer.device_id) && peer.device_id !== activePeerId,
      );
      if (nextPeer) setDiscoveryPromptPeerId(nextPeer.device_id);
    }
  }, [activePeerId, availableDiscoveredPeers, discoveryPromptPeerId, dismissedDiscoveryPeerIds, hasClientSession, isController, trustedDeviceIds]);

  useEffect(() => {
    if (isController && !controllerPromptDeviceId && connectionState?.pending_pairing_requests.length) {
      setControllerPromptDeviceId(connectionState.pending_pairing_requests[0].device_id);
    }
    if (controllerPromptDeviceId && !connectionState?.pending_pairing_requests.some((request) => request.device_id === controllerPromptDeviceId)) {
      setControllerPromptDeviceId(null);
    }
  }, [connectionState?.pending_pairing_requests, controllerPromptDeviceId, isController]);

  async function handleCopy(text: string, type: string) {
    await navigator.clipboard.writeText(text);
    setCopied(type);
    window.setTimeout(() => setCopied(null), 1800);
  }

  async function handleGeneratePairing() {
    try {
      setError("");
      const offer = await createPairingOffer();
      setPairingCode(offer.pairing_code);
      setConnectionState(await getConnectionState());
      setStatusText("主控端配对码已刷新");
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function handleSaveName() {
    try {
      setError("");
      const next = await updateDeviceProfile(machineName);
      setProfile(next);
      setMachineName(next.display_name);
      setIsEditingName(false);
      setStatusText(`设备名称已更新为 ${next.display_name}`);
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function handleMoveScreen(id: string, newPosition: number) {
    if (!topology) return;
    try {
      setError("");
      const x = newPosition % topology.grid_width;
      const y = Math.floor(newPosition / topology.grid_width);
      setTopology(await placeTopologyDevice(id, { x, y }));
      setStatusText("拓扑布局已更新");
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function handleRequestDiscoveryPairing(peer: DiscoveryPeer) {
    try {
      setError("");
      const issuedPairingCode = await submitDiscoveryPairingRequest(peer.device_id);
      setConnectionState(await getConnectionState());
      setDismissedDiscoveryPeerIds((current) => current.filter((id) => id !== peer.device_id));
      setDiscoveryPromptPeerId(null);
      setStatusText(`已向 ${peer.display_name} 发起连接请求，等待主控端确认。请求标识：${issuedPairingCode}`);
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function handlePendingPairing(deviceId: string, accept: boolean) {
    try {
      setError("");
      setConnectionState(await respondToPendingPairing(deviceId, accept));
      const [nextDevices, nextTopology, nextPeers] = await Promise.all([getDeviceManagementSnapshot(), getTopologySnapshot(), listDiscoveredPeers()]);
      setDeviceManagement(nextDevices);
      setTopology(nextTopology);
      setDiscoveredPeers(nextPeers);
      setControllerPromptDeviceId(null);
      setStatusText(accept ? "已确认连接请求，并同步到主控拓扑" : "已拒绝该连接请求");
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function handleSetRole(role: "controller" | "client") {
    try {
      setError("");
      setConnectionState(await setAppRole(role));
      setStatusText(role === "controller" ? "已切换为主控端" : "已切换为被控端");
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function handleToggleControllerService(enabled: boolean) {
    try {
      setError("");
      setConnectionState(await setControllerServiceEnabled(enabled));
      const [nextDevices, nextTopology, nextPeers] = await Promise.all([getDeviceManagementSnapshot(), getTopologySnapshot(), listDiscoveredPeers()]);
      setDeviceManagement(nextDevices);
      setTopology(nextTopology);
      setDiscoveredPeers(nextPeers);
      setStatusText(enabled ? "主控端服务已启用，已尝试恢复已配对设备连接状态" : "主控端服务已停止");
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function handleManualConnect() {
    try {
      setError("");
      const result = await connectToManualEndpoint(host.trim(), Number(port), manualPairingCode.trim());
      const [nextTopology, nextState, nextPeers] = await Promise.all([getTopologySnapshot(), getConnectionState(), listDiscoveredPeers()]);
      setDeviceManagement(result.device_management);
      setTopology(nextTopology);
      setConnectionState(nextState);
      setDiscoveredPeers(nextPeers);
      setStatusText(`已提交手动配对请求到主控端 ${result.endpoint_host}:${result.session_port}`);
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function handleDisconnect() {
    try {
      setError("");
      setConnectionState(await disconnectActivePeer());
      setStatusText("已断开当前连接");
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function handleRemovePairedDevice(deviceId: string) {
    try {
      setError("");
      setDeviceManagement(await repairManagedDevice(deviceId, "revoke"));
      const [nextTopology, nextState, nextPeers] = await Promise.all([getTopologySnapshot(), getConnectionState(), listDiscoveredPeers()]);
      setTopology(nextTopology);
      setConnectionState(nextState);
      setDiscoveredPeers(nextPeers);
      setStatusText("已删除配对设备");
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  return (
    <div className="flex h-full w-full animate-in gap-4 fade-in slide-in-from-bottom-4 duration-500">
      <div className="flex flex-1 flex-col overflow-y-auto rounded-xl border border-slate-700/60 bg-slate-800/60 p-4 shadow-inner">
        <div className="mb-5 flex shrink-0 rounded-lg border border-slate-700 bg-slate-900 p-1 shadow-sm">
          <button className={`flex-1 rounded-md py-1.5 text-xs font-medium transition-all ${isController ? "bg-blue-600 text-white shadow" : "text-slate-400 hover:text-white"}`} onClick={() => void handleSetRole("controller")} type="button">主控端</button>
          <button className={`flex-1 rounded-md py-1.5 text-xs font-medium transition-all ${!isController ? "bg-blue-600 text-white shadow" : "text-slate-400 hover:text-white"}`} disabled={controllerServiceEnabled} onClick={() => void handleSetRole("client")} type="button">被控端</button>
        </div>

        {isController ? (
          <div className="flex flex-1 flex-col justify-center space-y-3">
            <div className="flex gap-2">
              <button className="flex flex-1 items-center justify-center gap-2 rounded-md bg-emerald-600 px-3 py-2 text-xs font-medium text-white transition-colors hover:bg-emerald-500 disabled:bg-slate-700" disabled={controllerServiceEnabled} onClick={() => void handleToggleControllerService(true)} type="button"><PlayCircle size={14} />启用主控端服务</button>
              <button className={`flex flex-1 items-center justify-center gap-2 rounded-md px-3 py-2 text-xs font-medium transition-colors ${controllerServiceEnabled ? "bg-rose-600 text-white hover:bg-rose-500" : "bg-slate-700 text-slate-100 hover:bg-slate-600 disabled:bg-slate-800 disabled:text-slate-500"}`} disabled={!controllerServiceEnabled} onClick={() => void handleToggleControllerService(false)} type="button"><PauseCircle size={14} />停止主控端服务</button>
            </div>

            <div>
              <h3 className="mb-2 flex items-center gap-1.5 text-xs font-semibold text-white"><Server className="text-blue-400" size={14} />本机网络信息</h3>
              <div className="space-y-1.5">
                <div className="flex items-center justify-between rounded-lg border border-slate-700/60 bg-slate-900/80 p-2">
                  <div className="mr-2 flex-1">
                    <span className="mb-0.5 block text-[10px] text-slate-500">设备名称</span>
                    {isEditingName ? (
                      <div className="flex items-center gap-2">
                        <input autoFocus className="w-full rounded border border-slate-600 bg-slate-800 px-1.5 py-0.5 text-xs text-white focus:border-blue-500 focus:outline-none" onChange={(event) => setMachineName(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void handleSaveName(); }} value={machineName} />
                        <button className="text-green-500 hover:text-green-400" onClick={() => void handleSaveName()} type="button"><Check size={14} /></button>
                      </div>
                    ) : (
                      <div className="group flex items-center justify-between">
                        <span className="font-mono text-xs text-blue-400">{profile?.display_name ?? machineName}</span>
                        <button className="text-slate-400 opacity-0 transition-opacity hover:text-white group-hover:opacity-100" onClick={() => setIsEditingName(true)} type="button"><Edit2 size={12} /></button>
                      </div>
                    )}
                  </div>
                </div>
                <div className="flex items-center justify-between rounded-lg border border-slate-700/60 bg-slate-900/80 p-2">
                  <div><span className="mb-0.5 block text-[10px] text-slate-500">局域网 IP</span><span className="font-mono text-xs text-blue-400">{profile?.lan_ip ?? "--"}</span></div>
                  <button className="rounded-md bg-slate-800 p-1.5 text-slate-400 transition-colors hover:bg-slate-700 hover:text-white" onClick={() => profile && void handleCopy(profile.lan_ip, "ip")} title="复制 IP" type="button">{copied === "ip" ? <CheckCircle2 className="text-green-500" size={14} /> : <Copy size={14} />}</button>
                </div>
                <div className="flex items-center justify-between rounded-lg border border-slate-700/60 bg-slate-900/80 p-2">
                  <div><span className="mb-0.5 block text-[10px] text-slate-500">服务端口</span><span className="font-mono text-xs text-blue-400">{profile?.session_port ?? 24801}</span></div>
                  <button className="rounded-md bg-slate-800 p-1.5 text-slate-400 transition-colors hover:bg-slate-700 hover:text-white" onClick={() => void handleCopy(String(profile?.session_port ?? 24801), "port")} title="复制端口" type="button">{copied === "port" ? <CheckCircle2 className="text-green-500" size={14} /> : <Copy size={14} />}</button>
                </div>
              </div>
            </div>

            <div className="pt-1">
              <h3 className="mb-2 text-xs font-semibold text-white">主控端配对码</h3>
              <div className="flex items-center justify-between rounded-lg border border-slate-700/60 bg-slate-900/80 p-2.5">
                <span className="font-mono text-sm font-bold tracking-widest text-emerald-400">{pairingCode || "------"}</span>
                <div className="flex gap-1.5">
                  <button className="rounded-md bg-slate-800 p-1.5 text-slate-400 transition-colors hover:bg-slate-700 hover:text-white" onClick={() => void handleGeneratePairing()} title="刷新配对码" type="button"><RefreshCw size={14} /></button>
                  <button className="rounded-md bg-slate-800 p-1.5 text-slate-400 transition-colors hover:bg-slate-700 hover:text-white" onClick={() => pairingCode && void handleCopy(pairingCode, "pairing-code")} title="复制配对码" type="button">{copied === "pairing-code" ? <CheckCircle2 className="text-emerald-500" size={14} /> : <Copy size={14} />}</button>
                </div>
              </div>
              <p className="mt-2 text-[10px] text-slate-500">手动配对时，只需要在被控端输入主控端的 IP、端口和这个配对码，不需要双方都输入。</p>
            </div>

            {connectionState?.pending_pairing_requests.length ? <div className="space-y-2 rounded-lg border border-slate-700/60 bg-slate-900/70 p-3"><div className="text-[10px] text-slate-400">待确认连接请求</div>{connectionState.pending_pairing_requests.map((request) => <div className="rounded-md bg-slate-800/70 p-2" key={request.device_id}><div className="text-[11px] text-slate-200">{request.display_name}</div><div className="mt-0.5 text-[10px] text-slate-500">{request.address}:{request.port}</div><div className="mt-0.5 text-[10px] text-slate-500">请求码：{request.pairing_code}</div><div className="mt-2 flex gap-2"><button className="rounded-md bg-blue-600 px-2 py-1 text-[10px] text-white transition-colors hover:bg-blue-500" onClick={() => void handlePendingPairing(request.device_id, true)} type="button">确认连接</button><button className="rounded-md bg-slate-700 px-2 py-1 text-[10px] text-slate-200 transition-colors hover:bg-slate-600" onClick={() => void handlePendingPairing(request.device_id, false)} type="button">拒绝</button></div></div>)}</div> : null}

            {deviceManagement?.devices.length ? <div className="space-y-1 rounded-lg border border-slate-700/60 bg-slate-900/70 p-2"><div className="text-[10px] text-slate-400">已配对设备</div>{deviceManagement.devices.map((device) => <div className="flex items-center justify-between rounded-md bg-slate-800/70 px-2 py-1" key={device.device_id}><div className="min-w-0"><div className="truncate text-[11px] text-slate-200">{device.display_name}</div><div className="text-[10px] text-slate-500">{deviceStatusLabel(device.status)}</div></div><div className="flex items-center gap-2">{activePeerId === device.device_id && isActivePeerConnected ? <span className="rounded-full bg-emerald-400/10 px-2 py-0.5 text-[10px] text-emerald-400">已连接</span> : null}<button className="rounded-md bg-rose-500/10 px-2 py-1 text-[10px] text-rose-300 transition-colors hover:bg-rose-500/20" onClick={() => void handleRemovePairedDevice(device.device_id)} type="button">删除</button></div></div>)}</div> : null}

            <div className="mt-auto rounded-md border border-emerald-500/20 bg-emerald-400/10 p-2 text-[10px] text-emerald-400">{statusText}</div>
          </div>
        ) : (
          <div className="flex flex-1 flex-col justify-center space-y-4">
            <div className="rounded-md border border-blue-500/20 bg-blue-500/10 p-3 text-[10px] text-blue-300">自动发现模式下，被控端会主动发现主控端并发起连接请求，主控端确认后完成配对和连接。</div>

            {availableDiscoveredPeers.length > 0 ? <div className="space-y-2 rounded-lg border border-slate-700/60 bg-slate-900/70 p-3"><div className="text-[10px] text-slate-400">自动发现到的主控端</div>{availableDiscoveredPeers.map((peer) => { const isTrusted = trustedDeviceIds.has(peer.device_id); const isPendingPeer = activePeerId === peer.device_id && connectionState?.active_peer_state === "pending"; const isConnectedPeer = activePeerId === peer.device_id && connectionState?.active_peer_state === "connected"; const label = isConnectedPeer ? "已连接" : isPendingPeer ? "等待确认" : isTrusted ? "已配对" : "请求连接"; return <div className="flex items-center justify-between rounded-md bg-slate-800/70 px-2 py-2" key={peer.device_id}><div className="min-w-0"><div className="truncate text-[11px] text-slate-200">{peer.display_name}</div><div className="text-[10px] text-slate-500">{peer.address}:{peer.port}</div></div><button className="rounded-md bg-blue-600 px-2 py-1 text-[10px] text-white transition-colors hover:bg-blue-500 disabled:bg-slate-700" disabled={isTrusted || isPendingPeer || isConnectedPeer} onClick={() => void handleRequestDiscoveryPairing(peer)} type="button">{label}</button></div>; })}</div> : <div className="rounded-lg border border-dashed border-slate-700/60 bg-slate-900/40 p-3 text-[10px] text-slate-400">暂未发现可连接的主控端，请确认主控端已启用服务且处于同一局域网内。</div>}

            <div><h3 className="mb-3 flex items-center gap-1.5 text-xs font-semibold text-white"><Server className="text-blue-400" size={14} />手动配对</h3><p className="mb-3 text-[10px] text-slate-400">只需要输入主控端 IP、端口和配对码，被控端单向完成配对请求。</p></div>

            <div className="space-y-3">
              <div className="grid grid-cols-3 gap-2">
                <div className="col-span-2 space-y-1"><label className="text-[10px] text-slate-400">主控端 IP</label><input className="w-full rounded-md border border-slate-700 bg-slate-900 px-2.5 py-1.5 font-mono text-xs text-white transition-all focus:border-blue-500 focus:outline-none" onChange={(event) => { setHost(event.target.value); setHasInitializedHost(true); }} placeholder="192.168.1.100" value={host} /></div>
                <div className="space-y-1"><label className="text-[10px] text-slate-400">端口</label><input className="w-full rounded-md border border-slate-700 bg-slate-900 px-2.5 py-1.5 font-mono text-xs text-white transition-all focus:border-blue-500 focus:outline-none" onChange={(event) => setPort(event.target.value)} placeholder="24801" value={port} /></div>
              </div>
              <div className="space-y-1"><label className="text-[10px] text-slate-400">主控端配对码</label><input className="w-full rounded-md border border-slate-700 bg-slate-900 px-2.5 py-1.5 font-mono text-xs font-bold uppercase tracking-widest text-emerald-400 transition-all focus:border-blue-500 focus:outline-none" onChange={(event) => setManualPairingCode(event.target.value)} placeholder="XXXXXX" value={manualPairingCode} /></div>
            </div>

            <div className="flex gap-2">
              <button className="mt-auto flex-1 rounded-md bg-blue-600 px-4 py-2 text-xs font-medium text-white shadow-sm transition-colors hover:bg-blue-500" onClick={() => void handleManualConnect()} type="button">发起配对</button>
              <button className="mt-auto flex items-center justify-center gap-2 rounded-md bg-slate-700 px-4 py-2 text-xs font-medium text-slate-100 transition-colors hover:bg-slate-600 disabled:bg-slate-800 disabled:text-slate-500" disabled={!activePeerId} onClick={() => void handleDisconnect()} type="button"><Unplug size={14} />断开连接</button>
            </div>

            <div className={`rounded-md border p-3 text-[10px] ${currentClientCard.className}`}><div className="text-[11px] font-medium">{currentClientCard.title}</div><div className="mt-1">{currentClientCard.detail}</div></div>
            <div className="mt-auto rounded-md border border-emerald-500/20 bg-emerald-400/10 p-2 text-[10px] text-emerald-400">{statusText}</div>
          </div>
        )}

        {error ? <div className="mt-3 text-[10px] text-red-400">{error}</div> : null}
      </div>

      {discoveryPromptPeer ? <div className="pointer-events-none absolute inset-0 z-20 flex items-center justify-center"><div className="pointer-events-auto w-[320px] rounded-xl border border-slate-700 bg-slate-900/95 p-4 shadow-2xl"><div className="text-sm font-semibold text-white">发现主控端设备</div><div className="mt-2 text-xs text-slate-300">{discoveryPromptPeer.display_name}</div><div className="mt-1 text-[10px] text-slate-500">{discoveryPromptPeer.address}:{discoveryPromptPeer.port}</div><p className="mt-3 text-[11px] leading-5 text-slate-400">是否向该主控端发起连接请求？主控端确认后会完成配对。</p><div className="mt-4 flex gap-2"><button className="flex-1 rounded-md bg-blue-600 px-3 py-2 text-xs font-medium text-white hover:bg-blue-500" onClick={() => void handleRequestDiscoveryPairing(discoveryPromptPeer)} type="button">请求连接</button><button className="flex-1 rounded-md bg-slate-700 px-3 py-2 text-xs font-medium text-slate-200 hover:bg-slate-600" onClick={() => { setDismissedDiscoveryPeerIds((current) => discoveryPromptPeer && !current.includes(discoveryPromptPeer.device_id) ? [...current, discoveryPromptPeer.device_id] : current); setDiscoveryPromptPeerId(null); setStatusText("已忽略该主控设备，本轮发现周期内不再弹出提示"); }} type="button">暂不连接</button></div></div></div> : null}

      {controllerPrompt ? <div className="pointer-events-none absolute inset-0 z-20 flex items-center justify-center"><div className="pointer-events-auto w-[320px] rounded-xl border border-slate-700 bg-slate-900/95 p-4 shadow-2xl"><div className="text-sm font-semibold text-white">收到连接请求</div><div className="mt-2 text-xs text-slate-300">{controllerPrompt.display_name}</div><div className="mt-1 text-[10px] text-slate-500">{controllerPrompt.address}:{controllerPrompt.port}</div><div className="mt-1 text-[10px] text-slate-500">请求码：{controllerPrompt.pairing_code}</div><p className="mt-3 text-[11px] leading-5 text-slate-400">确认后将该设备加入已配对列表，并自动显示到主控拓扑中。</p><div className="mt-4 flex gap-2"><button className="flex-1 rounded-md bg-blue-600 px-3 py-2 text-xs font-medium text-white hover:bg-blue-500" onClick={() => void handlePendingPairing(controllerPrompt.device_id, true)} type="button">确认连接</button><button className="flex-1 rounded-md bg-slate-700 px-3 py-2 text-xs font-medium text-slate-200 hover:bg-slate-600" onClick={() => void handlePendingPairing(controllerPrompt.device_id, false)} type="button">拒绝</button></div></div></div> : null}

      {isController ? (
        <div className="relative flex flex-1 flex-col overflow-hidden rounded-xl border border-slate-700/60 bg-slate-800/60 p-4 shadow-inner">
          <h3 className="mb-1 text-xs font-semibold text-white">拓扑结构</h3>
          <p className="mb-4 text-[10px] text-slate-400">配对成功后的被控设备会自动加入这里，并可通过拖拽调整相对位置。</p>
          <div className="flex flex-1 flex-col items-center justify-center">
            <div className="relative">
              <div className="absolute -top-4 left-1/2 -translate-x-1/2 transform text-[9px] text-slate-600">上边</div>
              <div className="absolute -bottom-4 left-1/2 -translate-x-1/2 transform text-[9px] text-slate-600">下边</div>
              <div className="absolute top-1/2 -left-7 -translate-y-1/2 -rotate-90 transform whitespace-nowrap text-[9px] text-slate-600">左边</div>
              <div className="absolute top-1/2 -right-7 -translate-y-1/2 rotate-90 transform whitespace-nowrap text-[9px] text-slate-600">右边</div>
              <div className="grid grid-cols-3 gap-2 rounded-2xl border border-slate-700/30 bg-slate-900/30 p-3">
                {Array.from({ length: 9 }).map((_, pos) => {
                  const screen = screens.find((item) => item.position === pos);
                  return <GridCell key={pos} onMoveScreen={handleMoveScreen} position={pos} screen={screen} />;
                })}
              </div>
            </div>
          </div>
        </div>
      ) : <div className="pointer-events-none hidden flex-1 opacity-0 md:block" />}
    </div>
  );
}
