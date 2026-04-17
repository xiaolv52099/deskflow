import { useEffect, useMemo, useState } from "react";
import { useDrag, useDrop } from "react-dnd";
import {
  Check,
  CheckCircle2,
  Copy,
  Edit2,
  Monitor,
  MonitorSmartphone,
  RefreshCw,
  Server,
} from "lucide-react";
import {
  connectToManualEndpoint,
  createPairingOffer,
  getDeviceManagementSnapshot,
  getDeviceProfile,
  getTopologySnapshot,
  listDiscoveredPeers,
  placeTopologyDevice,
  trustDiscoveredPeer,
  updateDeviceProfile,
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

function ScreenBox({ id, name, isMaster, position }: ScreenBoxProps) {
  const [{ isDragging }, drag] = useDrag(() => ({
    type: ItemType,
    item: { id, position },
    collect: (monitor) => ({
      isDragging: monitor.isDragging(),
    }),
  }));

  return (
    <div
      ref={(node) => {
        drag(node);
      }}
      className={`flex h-full w-full cursor-grab select-none flex-col items-center justify-center rounded-lg border p-1 shadow-sm transition-all active:cursor-grabbing ${
        isMaster
          ? "border-blue-500 bg-blue-900/60 text-blue-100 shadow-blue-900/20"
          : "border-slate-600 bg-slate-800 text-slate-200 hover:border-slate-500"
      } ${isDragging ? "scale-95 opacity-40" : "opacity-100 hover:scale-[1.02]"}`}
    >
      {isMaster ? (
        <Monitor className="mb-1 text-blue-400" size={20} />
      ) : (
        <MonitorSmartphone className="mb-1 text-slate-400" size={16} />
      )}
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
    collect: (monitor) => ({
      isOver: monitor.isOver(),
    }),
  }));

  return (
    <div
      ref={(node) => {
        drop(node);
      }}
      className={`flex h-[68px] w-[68px] items-center justify-center rounded-xl border border-dashed p-1 transition-all ${
        isOver
          ? "scale-105 border-solid border-blue-400 bg-blue-500/10 shadow-inner"
          : "border-slate-700/60 bg-slate-900/40 hover:bg-slate-800/60"
      }`}
    >
      {screen ? (
        <ScreenBox {...screen} />
      ) : (
        <span className="font-mono text-sm font-bold text-slate-700 opacity-30">{position + 1}</span>
      )}
    </div>
  );
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
  return status;
}

export function ConnectionConfig() {
  const [isMaster, setIsMaster] = useState(true);
  const [copied, setCopied] = useState<string | null>(null);
  const [isEditingName, setIsEditingName] = useState(false);
  const [machineName, setMachineName] = useState("Deskflow-Plus");
  const [profile, setProfile] = useState<DeviceProfileDto | null>(null);
  const [pairingOffer, setPairingOffer] = useState("");
  const [pairingCode, setPairingCode] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("24801");
  const [manualPairingCode, setManualPairingCode] = useState("");
  const [topology, setTopology] = useState<TopologySnapshot | null>(null);
  const [deviceManagement, setDeviceManagement] = useState<DeviceManagementSnapshot | null>(null);
  const [discoveredPeers, setDiscoveredPeers] = useState<DiscoveryPeer[]>([]);
  const [statusText, setStatusText] = useState("正在初始化服务...");
  const [error, setError] = useState("");

  async function refresh() {
    const [nextProfile, nextTopology, nextDevices, nextPeers, nextOffer] = await Promise.all([
      getDeviceProfile(),
      getTopologySnapshot(),
      getDeviceManagementSnapshot(),
      listDiscoveredPeers(),
      createPairingOffer(),
    ]);

    setProfile(nextProfile);
    setMachineName(nextProfile.display_name);
    setHost(nextProfile.lan_ip);
    setPairingOffer(nextOffer.payload);
    setPairingCode(nextOffer.pairing_code);
    setTopology(nextTopology);
    setDeviceManagement(nextDevices);
    setDiscoveredPeers(nextPeers);
    setStatusText(`服务运行中，监听 ${nextProfile.lan_ip}:${nextProfile.session_port}`);
  }

  useEffect(() => {
    void refresh().catch((nextError) => setError(formatError(nextError)));
  }, []);

  const screens = useMemo(() => {
    if (!topology) return [];
    return topology.devices
      .filter((device) => device.position)
      .map((device) => ({
        id: device.device_id,
        name: device.display_name,
        isMaster: device.device_id === topology.controller_device_id,
        position: (device.position!.y * topology.grid_width) + device.position!.x,
      }));
  }, [topology]);

  async function handleCopy(text: string, type: string) {
    await navigator.clipboard.writeText(text);
    setCopied(type);
    window.setTimeout(() => setCopied(null), 1800);
  }

  async function handleGeneratePairing() {
    try {
      setError("");
      const offer = await createPairingOffer();
      setPairingOffer(offer.payload);
      setPairingCode(offer.pairing_code);
      setStatusText("已刷新本机配对信息");
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
      const next = await placeTopologyDevice(id, { x, y });
      setTopology(next);
      setStatusText("屏幕布局已更新");
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function handleTrustDiscovered(peer: DiscoveryPeer) {
    try {
      setError("");
      const nextDevices = await trustDiscoveredPeer(peer.device_id);
      setDeviceManagement(nextDevices);
      const nextTopology = await getTopologySnapshot();
      setTopology(nextTopology);
      const nextPeers = await listDiscoveredPeers();
      setDiscoveredPeers(nextPeers);
      setStatusText(`已信任设备 ${peer.display_name}`);
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function handleManualConnect() {
    try {
      setError("");
      const result = await connectToManualEndpoint(host.trim(), Number(port), manualPairingCode.trim());
      setPairingOffer(result.imported_payload);
      setDeviceManagement(result.device_management);
      const nextTopology = await getTopologySnapshot();
      setTopology(nextTopology);
      setStatusText(`已写入手动配对配置 ${result.endpoint_host}:${result.session_port}`);
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  const trustedDeviceIds = new Set(deviceManagement?.devices.map((device) => device.device_id) ?? []);

  return (
    <div className="flex h-full w-full animate-in gap-4 fade-in slide-in-from-bottom-4 duration-500">
      <div className="flex flex-1 flex-col overflow-y-auto rounded-xl border border-slate-700/60 bg-slate-800/60 p-4 shadow-inner">
        <div className="mb-5 flex shrink-0 rounded-lg border border-slate-700 bg-slate-900 p-1 shadow-sm">
          <button
            className={`flex-1 rounded-md py-1.5 text-xs font-medium transition-all ${
              isMaster ? "bg-blue-600 text-white shadow" : "text-slate-400 hover:text-white"
            }`}
            onClick={() => setIsMaster(true)}
            type="button"
          >
            主控端
          </button>
          <button
            className={`flex-1 rounded-md py-1.5 text-xs font-medium transition-all ${
              !isMaster ? "bg-blue-600 text-white shadow" : "text-slate-400 hover:text-white"
            }`}
            onClick={() => setIsMaster(false)}
            type="button"
          >
            被控端
          </button>
        </div>

        {isMaster ? (
          <div className="flex flex-1 flex-col justify-center space-y-3">
            <div>
              <h3 className="mb-2 flex items-center gap-1.5 text-xs font-semibold text-white">
                <Server className="text-blue-400" size={14} />
                本机网络信息
              </h3>
              <div className="space-y-1.5">
                <div className="flex items-center justify-between rounded-lg border border-slate-700/60 bg-slate-900/80 p-2">
                  <div className="mr-2 flex-1">
                    <span className="mb-0.5 block text-[10px] text-slate-500">设备名称</span>
                    {isEditingName ? (
                      <div className="flex items-center gap-2">
                        <input
                          autoFocus
                          className="w-full rounded border border-slate-600 bg-slate-800 px-1.5 py-0.5 text-xs text-white focus:border-blue-500 focus:outline-none"
                          onChange={(event) => setMachineName(event.target.value)}
                          onKeyDown={(event) => {
                            if (event.key === "Enter") void handleSaveName();
                          }}
                          value={machineName}
                        />
                        <button className="text-green-500 hover:text-green-400" onClick={() => void handleSaveName()} type="button">
                          <Check size={14} />
                        </button>
                      </div>
                    ) : (
                      <div className="group flex items-center justify-between">
                        <span className="font-mono text-xs text-blue-400">{profile?.display_name ?? machineName}</span>
                        <button
                          className="text-slate-400 opacity-0 transition-opacity hover:text-white group-hover:opacity-100"
                          onClick={() => setIsEditingName(true)}
                          type="button"
                        >
                          <Edit2 size={12} />
                        </button>
                      </div>
                    )}
                  </div>
                </div>

                <div className="flex items-center justify-between rounded-lg border border-slate-700/60 bg-slate-900/80 p-2">
                  <div>
                    <span className="mb-0.5 block text-[10px] text-slate-500">局域网 IP</span>
                    <span className="font-mono text-xs text-blue-400">{profile?.lan_ip ?? "--"}</span>
                  </div>
                  <button
                    className="rounded-md bg-slate-800 p-1.5 text-slate-400 transition-colors hover:bg-slate-700 hover:text-white"
                    onClick={() => profile && void handleCopy(profile.lan_ip, "ip")}
                    title="复制 IP"
                    type="button"
                  >
                    {copied === "ip" ? <CheckCircle2 className="text-green-500" size={14} /> : <Copy size={14} />}
                  </button>
                </div>

                <div className="flex items-center justify-between rounded-lg border border-slate-700/60 bg-slate-900/80 p-2">
                  <div>
                    <span className="mb-0.5 block text-[10px] text-slate-500">服务端口</span>
                    <span className="font-mono text-xs text-blue-400">{profile?.session_port ?? 24801}</span>
                  </div>
                  <button
                    className="rounded-md bg-slate-800 p-1.5 text-slate-400 transition-colors hover:bg-slate-700 hover:text-white"
                    onClick={() => void handleCopy(String(profile?.session_port ?? 24801), "port")}
                    title="复制端口"
                    type="button"
                  >
                    {copied === "port" ? <CheckCircle2 className="text-green-500" size={14} /> : <Copy size={14} />}
                  </button>
                </div>
              </div>
            </div>

            <div className="pt-1">
              <h3 className="mb-2 text-xs font-semibold text-white">动态配对码</h3>
              <div className="flex items-center justify-between rounded-lg border border-slate-700/60 bg-slate-900/80 p-2.5">
                <span className="font-mono text-sm font-bold tracking-widest text-emerald-400">{pairingCode || "----"}</span>
                <div className="flex gap-1.5">
                  <button
                    className="rounded-md bg-slate-800 p-1.5 text-slate-400 transition-colors hover:bg-slate-700 hover:text-white"
                    onClick={() => void handleGeneratePairing()}
                    title="刷新配对码"
                    type="button"
                  >
                    <RefreshCw size={14} />
                  </button>
                  <button
                    className="rounded-md bg-slate-800 p-1.5 text-slate-400 transition-colors hover:bg-slate-700 hover:text-white"
                    onClick={() => pairingOffer && void handleCopy(pairingOffer, "payload")}
                    title="复制完整配对信息"
                    type="button"
                  >
                    {copied === "payload" ? <CheckCircle2 className="text-emerald-500" size={14} /> : <Copy size={14} />}
                  </button>
                </div>
              </div>
            </div>

            {discoveredPeers.length > 0 ? (
              <div className="space-y-1 rounded-lg border border-slate-700/60 bg-slate-900/70 p-2">
                <div className="text-[10px] text-slate-400">自动发现设备</div>
                {discoveredPeers.slice(0, 4).map((peer) => (
                  <div className="flex items-center justify-between rounded-md bg-slate-800/70 px-2 py-1" key={peer.device_id}>
                    <div className="min-w-0">
                      <div className="truncate text-[11px] text-slate-200">{peer.display_name}</div>
                      <div className="text-[10px] text-slate-500">
                        {peer.address}:{peer.port}
                      </div>
                    </div>
                    <button
                      className="rounded-md bg-blue-600 px-2 py-1 text-[10px] text-white transition-colors hover:bg-blue-500 disabled:bg-slate-700"
                      disabled={trustedDeviceIds.has(peer.device_id)}
                      onClick={() => void handleTrustDiscovered(peer)}
                      type="button"
                    >
                      {trustedDeviceIds.has(peer.device_id) ? "已信任" : "信任"}
                    </button>
                  </div>
                ))}
              </div>
            ) : (
              <div className="rounded-lg border border-dashed border-slate-700/60 bg-slate-900/40 p-3 text-[10px] text-slate-400">
                暂未发现其他局域网设备，请确认双方已启动应用且处于同一网段。
              </div>
            )}

            {deviceManagement?.devices.length ? (
              <div className="space-y-1 rounded-lg border border-slate-700/60 bg-slate-900/70 p-2">
                <div className="text-[10px] text-slate-400">已配对设备</div>
                {deviceManagement.devices.map((device) => (
                  <div className="flex items-center justify-between rounded-md bg-slate-800/70 px-2 py-1" key={device.device_id}>
                    <div className="min-w-0">
                      <div className="truncate text-[11px] text-slate-200">{device.display_name}</div>
                      <div className="text-[10px] text-slate-500">{deviceStatusLabel(device.status)}</div>
                    </div>
                  </div>
                ))}
              </div>
            ) : null}

            <div className="mt-auto rounded-md border border-emerald-500/20 bg-emerald-400/10 p-2 text-[10px] text-emerald-400">
              {statusText}
            </div>
          </div>
        ) : (
          <div className="flex flex-1 flex-col justify-center space-y-4">
            <div>
              <h3 className="mb-3 flex items-center gap-1.5 text-xs font-semibold text-white">
                <Server className="text-blue-400" size={14} />
                连接到主控端
              </h3>
              <p className="text-[10px] text-slate-400">输入主控端 IP、端口和配对码，建立可信连接。</p>
            </div>

            <div className="space-y-3">
              <div className="grid grid-cols-3 gap-2">
                <div className="col-span-2 space-y-1">
                  <label className="text-[10px] text-slate-400">主控端 IP</label>
                  <input
                    className="w-full rounded-md border border-slate-700 bg-slate-900 px-2.5 py-1.5 font-mono text-xs text-white transition-all focus:border-blue-500 focus:outline-none"
                    onChange={(event) => setHost(event.target.value)}
                    placeholder="192.168.1.100"
                    value={host}
                  />
                </div>
                <div className="space-y-1">
                  <label className="text-[10px] text-slate-400">端口</label>
                  <input
                    className="w-full rounded-md border border-slate-700 bg-slate-900 px-2.5 py-1.5 font-mono text-xs text-white transition-all focus:border-blue-500 focus:outline-none"
                    onChange={(event) => setPort(event.target.value)}
                    placeholder="24801"
                    value={port}
                  />
                </div>
              </div>

              <div className="space-y-1">
                <label className="text-[10px] text-slate-400">配对码</label>
                <input
                  className="w-full rounded-md border border-slate-700 bg-slate-900 px-2.5 py-1.5 font-mono text-xs font-bold uppercase tracking-widest text-emerald-400 transition-all focus:border-blue-500 focus:outline-none"
                  onChange={(event) => setManualPairingCode(event.target.value)}
                  placeholder="XXXXXX"
                  value={manualPairingCode}
                />
              </div>
            </div>

            <button
              className="mt-auto w-full rounded-md bg-blue-600 px-4 py-2 text-xs font-medium text-white shadow-sm transition-colors hover:bg-blue-500"
              onClick={() => void handleManualConnect()}
              type="button"
            >
              连接主控端
            </button>
          </div>
        )}

        {error ? <div className="mt-3 text-[10px] text-red-400">{error}</div> : null}
      </div>

      {isMaster ? (
        <div className="relative flex flex-1 flex-col overflow-hidden rounded-xl border border-slate-700/60 bg-slate-800/60 p-4 shadow-inner">
          <h3 className="mb-1 text-xs font-semibold text-white">屏幕拓扑布局</h3>
          <p className="mb-4 text-[10px] text-slate-400">拖拽设备到 3 x 3 网格中，确定副屏相对主屏的位置关系。</p>

          <div className="flex flex-1 flex-col items-center justify-center">
            <div className="relative">
              <div className="absolute -top-4 left-1/2 -translate-x-1/2 transform text-[9px] text-slate-600">上边界</div>
              <div className="absolute -bottom-4 left-1/2 -translate-x-1/2 transform text-[9px] text-slate-600">下边界</div>
              <div className="absolute top-1/2 -left-7 -translate-y-1/2 -rotate-90 transform whitespace-nowrap text-[9px] text-slate-600">左边界</div>
              <div className="absolute top-1/2 -right-7 -translate-y-1/2 rotate-90 transform whitespace-nowrap text-[9px] text-slate-600">右边界</div>

              <div className="grid grid-cols-3 gap-2 rounded-2xl border border-slate-700/30 bg-slate-900/30 p-3">
                {Array.from({ length: 9 }).map((_, pos) => {
                  const screen = screens.find((item) => item.position === pos);
                  return <GridCell key={pos} onMoveScreen={handleMoveScreen} position={pos} screen={screen} />;
                })}
              </div>
            </div>
          </div>
        </div>
      ) : (
        <div className="pointer-events-none hidden flex-1 opacity-0 md:block" />
      )}
    </div>
  );
}
