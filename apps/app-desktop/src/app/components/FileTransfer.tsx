import { useEffect, useMemo, useState } from "react";
import { Check, Copy, File, FileArchive, FileImage, FileSpreadsheet, FileText, FolderOpen, Paperclip, RefreshCw, Send, Upload } from "lucide-react";
import {
  createTransferPlan,
  getConnectionState,
  getDeviceManagementSnapshot,
  getTransferArtifactPath,
  listTransferPlans,
  revealTransferArtifactLocation,
  selectTransferFiles,
  type ConnectionStateDto,
  type DeviceManagementSnapshot,
  type SelectedTransferFileDto,
  type TransferRecord,
} from "../lib/tauri";

type PendingFile = SelectedTransferFileDto;

interface TransferRow {
  transfer_id: string;
  created_at_unix_ms: number;
  confirmed_at_unix_ms?: number | null;
  elapsed_ms: number;
  status: string;
  delivery_state: string;
  delivery_message?: string | null;
  direction: string;
  peer_display_name?: string | null;
  verified_files: number;
  total_bytes: number;
  file_name: string;
  file_size_bytes: number;
}

function formatBytes(size: number) {
  if (size >= 1024 * 1024 * 1024) return `${(size / 1024 / 1024 / 1024).toFixed(1)} GB`;
  if (size >= 1024 * 1024) return `${(size / 1024 / 1024).toFixed(1)} MB`;
  if (size >= 1024) return `${Math.round(size / 1024)} KB`;
  return `${size} B`;
}

function formatElapsed(ms: number) {
  if (ms >= 1000) return `${(ms / 1000).toFixed(2)} s`;
  return `${ms} ms`;
}

function formatStatus(status: string) {
  const normalized = status.toLowerCase();
  if (normalized === "completed") return "已完成";
  if (normalized === "inprogress") return "传输中";
  if (normalized === "approved") return "已批准";
  if (normalized === "pendingapproval") return "待确认";
  if (normalized === "cancelled") return "已取消";
  return status;
}

function formatDeliveryState(state: string) {
  const normalized = state.toLowerCase();
  if (normalized === "pending") return "待发送";
  if (normalized === "sending") return "发送中";
  if (normalized === "delivered") return "已送达";
  if (normalized === "received") return "已接收";
  if (normalized === "failed") return "失败";
  return state;
}

function formatTime(unixMs: number) {
  const date = new Date(unixMs);
  const now = new Date();
  const sameYear = date.getFullYear() === now.getFullYear();
  const sameDay = sameYear && date.toDateString() === now.toDateString();
  if (sameDay) {
    return new Intl.DateTimeFormat("zh-CN", { hour: "2-digit", minute: "2-digit" }).format(date);
  }
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

function fileTypeLabel(name: string) {
  const parts = name.split(".");
  if (parts.length < 2) return "文件";
  return parts[parts.length - 1].toUpperCase();
}

function fileIcon(name: string) {
  if (/\.(png|jpg|jpeg|gif|bmp|webp|svg)$/i.test(name)) return <FileImage className="text-indigo-400" size={16} />;
  if (/\.(zip|rar|7z|tar|gz)$/i.test(name)) return <FileArchive className="text-amber-400" size={16} />;
  if (/\.(xls|xlsx|csv)$/i.test(name)) return <FileSpreadsheet className="text-emerald-400" size={16} />;
  if (/\.(txt|md|doc|docx|pdf)$/i.test(name)) return <FileText className="text-blue-400" size={16} />;
  return <File className="text-slate-400" size={16} />;
}

function flattenRows(records: TransferRecord[]) {
  return records
    .slice()
    .sort((a, b) => b.created_at_unix_ms - a.created_at_unix_ms)
    .flatMap<TransferRow>((record) =>
      record.plan.manifest.files.map((file) => ({
        transfer_id: record.plan.manifest.transfer_id,
        created_at_unix_ms: record.created_at_unix_ms,
        confirmed_at_unix_ms: record.confirmed_at_unix_ms,
        elapsed_ms: record.elapsed_ms,
        status: record.progress.status,
        delivery_state: record.delivery_state,
        delivery_message: record.delivery_message,
        direction: record.direction,
        peer_display_name: record.peer_display_name,
        verified_files: record.verified_files,
        total_bytes: record.progress.total_bytes,
        file_name: file.name,
        file_size_bytes: file.size_bytes,
      })),
    );
}

function transferSummary(records: TransferRecord[]) {
  return records.reduce(
    (summary, record) => {
      summary.transfers += 1;
      summary.files += record.plan.manifest.files.length;
      summary.bytes += record.progress.total_bytes;
      return summary;
    },
    { transfers: 0, files: 0, bytes: 0 },
  );
}

function formatError(error: unknown) {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

export function FileTransfer() {
  const [records, setRecords] = useState<TransferRecord[]>([]);
  const [deviceSnapshot, setDeviceSnapshot] = useState<DeviceManagementSnapshot | null>(null);
  const [connectionState, setConnectionState] = useState<ConnectionStateDto | null>(null);
  const [pendingFiles, setPendingFiles] = useState<PendingFile[]>([]);
  const [statusText, setStatusText] = useState("请先确保双端已连接，然后选择要发送的文件。");
  const [error, setError] = useState("");
  const [isDragging, setIsDragging] = useState(false);
  const [isSending, setIsSending] = useState(false);
  const [copiedPath, setCopiedPath] = useState<string | null>(null);
  async function refresh() {
    const [nextRecords, nextDevices, nextConnectionState] = await Promise.all([
      listTransferPlans(),
      getDeviceManagementSnapshot(),
      getConnectionState(),
    ]);
    setRecords(nextRecords);
    setDeviceSnapshot(nextDevices);
    setConnectionState(nextConnectionState);
  }

  useEffect(() => {
    void refresh().catch((nextError) => setError(formatError(nextError)));
    const timer = window.setInterval(() => {
      void refresh().catch(() => undefined);
    }, 2500);
    return () => window.clearInterval(timer);
  }, []);

  const rows = useMemo(() => flattenRows(records), [records]);
  const summary = useMemo(() => transferSummary(records), [records]);
  const totalPendingBytes = useMemo(
    () => pendingFiles.reduce((sum, file) => sum + file.size_bytes, 0),
    [pendingFiles],
  );

  const activePeer = useMemo(() => {
    if (!connectionState?.active_peer_device_id) return null;
    return deviceSnapshot?.devices.find((device) => device.device_id === connectionState.active_peer_device_id) ?? null;
  }, [connectionState?.active_peer_device_id, deviceSnapshot?.devices]);

  const canSend = connectionState?.active_peer_state === "connected" && Boolean(activePeer);

  function updatePendingFiles(next: PendingFile[]) {
    if (!next.length) return;
    setPendingFiles(next);
    setStatusText(`已选择 ${next.length} 个文件，总大小 ${formatBytes(next.reduce((sum, file) => sum + file.size_bytes, 0))}`);
    setError("");
  }

  async function handlePickFiles() {
    if (isSending) return;
    try {
      setError("");
      const next = await selectTransferFiles();
      updatePendingFiles(next);
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function handleSendFiles() {
    if (isSending) return;
    if (!pendingFiles.length) {
      setError("请先选择要发送的文件。");
      return;
    }
    if (!canSend || !activePeer) {
      setError("当前没有有效的双端连接，无法发送文件。");
      return;
    }

    try {
      setError("");
      setIsSending(true);
      setStatusText(`正在向 ${activePeer.display_name} 发送 ${pendingFiles.length} 个文件，请不要关闭应用。`);
      const payload = pendingFiles.map((file) => ({
        name: file.name,
        path: file.path,
        size_bytes: file.size_bytes,
      }));
      const result = await createTransferPlan(activePeer.device_id, payload);
      setPendingFiles([]);
      setStatusText(result.delivery_message ?? `已向 ${activePeer.display_name} 发送 ${payload.length} 个文件。`);
      await refresh();
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setIsSending(false);
    }
  }

  async function handleCopyPath(row: TransferRow) {
    try {
      const path = await getTransferArtifactPath(row.transfer_id, row.file_name);
      await navigator.clipboard.writeText(path);
      setCopiedPath(`${row.transfer_id}:${row.file_name}`);
      window.setTimeout(() => setCopiedPath(null), 1500);
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function handleOpenLocation(row: TransferRow) {
    try {
      await revealTransferArtifactLocation(row.transfer_id, row.file_name);
    } catch (nextError) {
      setError(`打开位置失败：${formatError(nextError)}`);
    }
  }

  return (
    <div className="flex h-full w-full animate-in flex-col fade-in slide-in-from-bottom-4 duration-500">
      <div className="mb-3 flex items-center justify-between">
        <div>
          <h2 className="mb-0.5 text-lg font-bold">文件发送</h2>
          <p className="text-xs text-slate-400">补齐送达确认、失败状态和接收端回传后，记录才真正可信。</p>
        </div>
        <button
          className="flex items-center gap-1.5 rounded-md border border-slate-700 bg-slate-800 px-3 py-1.5 text-[11px] font-medium text-slate-200 transition-colors hover:bg-slate-700"
          onClick={() => void refresh()}
          type="button"
        >
          <RefreshCw size={12} />
          刷新
        </button>
      </div>

      <div className="flex flex-1 gap-4 overflow-hidden">
        <div className="flex w-[320px] shrink-0 flex-col overflow-hidden rounded-lg border border-slate-700 bg-slate-800/80 shadow-md">
          <div className="border-b border-slate-700 bg-slate-900/80 p-4">
            <h3 className="mb-1 text-sm font-semibold text-white">发送面板</h3>
            <p className="text-[11px] text-slate-400">当前连接：{canSend && activePeer ? activePeer.display_name : "未建立有效连接"}</p>
          </div>

          <div className="flex flex-1 flex-col p-4">
            <div
              className={`flex flex-1 flex-col items-center justify-center rounded-xl border border-dashed p-4 text-center transition-all ${
                isDragging ? "border-blue-400 bg-blue-500/10 text-blue-300" : "border-slate-700 bg-slate-900/40 text-slate-400"
              }`}
              onDragEnter={(event) => {
                event.preventDefault();
                setIsDragging(true);
              }}
              onDragLeave={(event) => {
                event.preventDefault();
                setIsDragging(false);
              }}
              onDragOver={(event) => {
                event.preventDefault();
                setIsDragging(true);
              }}
              onDrop={(event) => {
                event.preventDefault();
                setIsDragging(false);
                void handlePickFiles();
              }}
            >
              <Upload className="mb-3 text-blue-400" size={24} />
              <div className="text-sm font-medium text-slate-200">拖拽文件到这里</div>
              <div className="mt-1 text-[11px] text-slate-500">发送端改为 Rust 侧流式分片传输，避免整文件内存拷贝。</div>
            </div>

            <div className="mt-3 flex gap-2">
              <button
                className="flex flex-1 items-center justify-center gap-1.5 rounded-md bg-slate-700 px-3 py-2 text-xs font-medium text-slate-100 transition-colors hover:bg-slate-600"
                onClick={() => void handlePickFiles()}
                type="button"
              >
                <Paperclip size={14} />
                选择文件
              </button>
              <button
                className="flex flex-1 items-center justify-center gap-1.5 rounded-md bg-blue-600 px-3 py-2 text-xs font-medium text-white transition-colors hover:bg-blue-500 disabled:bg-slate-700 disabled:text-slate-400"
                disabled={!pendingFiles.length || !canSend || isSending}
                onClick={() => void handleSendFiles()}
                type="button"
              >
                <Send size={14} />
                {isSending ? "发送中" : "发送"}
              </button>
            </div>

            <div className="mt-4 rounded-lg border border-slate-700/60 bg-slate-900/60 p-3">
              <div className="mb-2 flex items-center justify-between">
                <span className="text-[11px] font-medium text-slate-200">待发送文件</span>
                <span className="text-[10px] text-slate-500">{pendingFiles.length} 项</span>
              </div>
              {pendingFiles.length === 0 ? (
                <div className="text-[11px] text-slate-500">还没有选择文件。</div>
              ) : (
                <div className="space-y-1.5">
                  {pendingFiles.slice(0, 6).map((file) => (
                    <div className="flex items-center gap-2 rounded-md bg-slate-800/70 px-2 py-1.5" key={`${file.name}-${file.size_bytes}`}>
                      {fileIcon(file.name)}
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-[11px] text-slate-200">{file.name}</div>
                        <div className="text-[10px] text-slate-500">
                          {fileTypeLabel(file.name)} · {formatBytes(file.size_bytes)}
                        </div>
                      </div>
                    </div>
                  ))}
                  <div className="pt-1 text-[10px] text-slate-400">总大小 {formatBytes(totalPendingBytes)}</div>
                </div>
              )}
            </div>

            <div className={`mt-3 rounded-md border p-2 text-[10px] ${canSend ? "border-emerald-500/20 bg-emerald-400/10 text-emerald-400" : "border-amber-500/20 bg-amber-400/10 text-amber-300"}`}>
              {statusText}
            </div>
            {error ? <div className="mt-2 text-[10px] text-red-400">{error}</div> : null}
          </div>
        </div>

        <div className="flex min-w-0 flex-1 flex-col overflow-hidden rounded-lg border border-slate-700 bg-slate-800/80 shadow-md">
          <div className="flex items-center justify-between border-b border-slate-700 bg-slate-900/70 px-4 py-3">
            <div>
              <h3 className="text-sm font-semibold text-white">发送记录</h3>
              <p className="text-[11px] text-slate-400">单条记录同时展示方向、送达状态、确认时间和结果说明。</p>
            </div>
            <div className="flex items-center gap-2 text-[11px] text-slate-400">
              <span>{summary.transfers} 次</span>
              <span>·</span>
              <span>{summary.files} 文件</span>
              <span>·</span>
              <span>{formatBytes(summary.bytes)}</span>
            </div>
          </div>

          <div className="flex-1 overflow-y-auto bg-slate-800/40 p-3">
            {rows.length === 0 ? (
              <div className="rounded-xl border border-dashed border-slate-700 bg-slate-900/40 p-4 text-sm text-slate-400">
                还没有发送记录。建立双端连接后，选择文件并点击“发送”，这里会显示确认后的真实结果。
              </div>
            ) : (
              <div className="space-y-2">
                {rows.map((row) => {
                  const copiedKey = `${row.transfer_id}:${row.file_name}`;
                  return (
                    <div className="flex items-center gap-3 rounded-lg border border-slate-700/70 bg-slate-900/60 px-3 py-2" key={copiedKey}>
                      <div className="shrink-0 rounded-md bg-slate-800/80 p-2">{fileIcon(row.file_name)}</div>
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-[12px] font-medium text-slate-100">{row.file_name}</div>
                        <div className="mt-0.5 flex flex-wrap items-center gap-x-2 text-[10px] text-slate-500">
                          <span>{row.direction === "inbound" ? "接收" : "发送"}</span>
                          {row.peer_display_name ? <span>{row.peer_display_name}</span> : null}
                          <span>{fileTypeLabel(row.file_name)}</span>
                          <span>{formatBytes(row.file_size_bytes)}</span>
                          <span>{formatTime(row.created_at_unix_ms)}</span>
                          {row.confirmed_at_unix_ms ? <span>确认 {formatTime(row.confirmed_at_unix_ms)}</span> : null}
                          <span>{formatElapsed(row.elapsed_ms)}</span>
                          <span>{formatStatus(row.status)}</span>
                          <span>{formatDeliveryState(row.delivery_state)}</span>
                          <span>校验 {row.verified_files}</span>
                          {row.delivery_message ? <span>{row.delivery_message}</span> : null}
                        </div>
                      </div>
                      <div className="flex shrink-0 items-center gap-1">
                        <button
                          className="flex items-center gap-1 rounded-md border border-slate-700 bg-slate-800 px-2 py-1 text-[10px] text-slate-300 transition-colors hover:bg-slate-700"
                          onClick={() => void handleOpenLocation(row)}
                          type="button"
                        >
                          <FolderOpen size={12} />
                          位置
                        </button>
                        <button
                          className="flex items-center gap-1 rounded-md border border-slate-700 bg-slate-800 px-2 py-1 text-[10px] text-slate-300 transition-colors hover:bg-slate-700"
                          onClick={() => void handleCopyPath(row)}
                          type="button"
                        >
                          {copiedPath === copiedKey ? <Check size={12} /> : <Copy size={12} />}
                          复制
                        </button>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
