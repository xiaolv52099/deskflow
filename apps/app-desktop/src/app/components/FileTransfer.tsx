import { useEffect, useRef, useState } from "react";
import {
  Download,
  FileText,
  FolderOpen,
  Image as ImageIcon,
  Monitor,
  MonitorSmartphone,
  Paperclip,
  RefreshCw,
  Send,
  Upload,
} from "lucide-react";
import { createTransferPlan, getDeviceManagementSnapshot, listTransferPlans, type TransferRecord } from "../lib/tauri";

interface PendingFile {
  name: string;
  size_bytes: number;
}

function formatBytes(size: number) {
  if (size >= 1024 * 1024 * 1024) return `${(size / 1024 / 1024 / 1024).toFixed(1)} GB`;
  if (size >= 1024 * 1024) return `${(size / 1024 / 1024).toFixed(1)} MB`;
  if (size >= 1024) return `${Math.round(size / 1024)} KB`;
  return `${size} B`;
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

function isImageFile(name: string) {
  return /\.(png|jpg|jpeg|gif|bmp|webp|svg)$/i.test(name);
}

export function FileTransfer() {
  const [records, setRecords] = useState<TransferRecord[]>([]);
  const [connectedCount, setConnectedCount] = useState(0);
  const [pairedDeviceName, setPairedDeviceName] = useState("");
  const [pendingFiles, setPendingFiles] = useState<PendingFile[]>([]);
  const [statusText, setStatusText] = useState("将文件拖拽到窗口中，或点击下方按钮选择文件。");
  const [error, setError] = useState("");
  const [isDragging, setIsDragging] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  async function refresh() {
    const [nextRecords, devices] = await Promise.all([listTransferPlans(), getDeviceManagementSnapshot()]);
    setRecords(nextRecords);
    setConnectedCount(devices.devices.length);
    const target = devices.devices.find((device) => device.status.toLowerCase() !== "revoked");
    setPairedDeviceName(target?.display_name ?? "");
  }

  useEffect(() => {
    void refresh();
  }, []);

  function updatePendingFromFileList(list: FileList | null) {
    if (!list || list.length === 0) return;
    const next = Array.from(list).map((file) => ({
      name: file.name,
      size_bytes: file.size,
    }));
    setPendingFiles(next);
    setStatusText(`已选择 ${next.length} 个文件，准备发送到已配对设备。`);
    setError("");
  }

  async function handleSendFiles() {
    if (pendingFiles.length === 0) {
      setError("请先选择要传输的文件。");
      return;
    }

    const devices = await getDeviceManagementSnapshot();
    const target = devices.devices.find((device) => device.status.toLowerCase() !== "revoked");
    if (!target) {
      setError("当前没有可用的已配对设备，无法开始传输。");
      return;
    }

    try {
      setError("");
      await createTransferPlan(target.device_id, pendingFiles);
      setStatusText(`已向 ${target.display_name} 发起 ${pendingFiles.length} 个文件的传输。`);
      setPendingFiles([]);
      if (fileInputRef.current) fileInputRef.current.value = "";
      await refresh();
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : String(nextError));
    }
  }

  const totalPendingBytes = pendingFiles.reduce((sum, file) => sum + file.size_bytes, 0);

  return (
    <div className="flex h-full w-full animate-in flex-col fade-in slide-in-from-bottom-4 duration-500">
      <div className="mb-3 flex items-center justify-between">
        <div>
          <h2 className="mb-0.5 text-lg font-bold">文件传输</h2>
          <p className="text-xs text-slate-400">在局域网配对设备之间发送文件，并查看最近的传输结果。</p>
        </div>
        <div className="flex items-center gap-2">
          <button
            className="flex items-center gap-1.5 rounded-md border border-slate-700 bg-slate-800 px-3 py-1.5 text-[11px] font-medium text-slate-200 transition-colors hover:bg-slate-700"
            onClick={() => void refresh()}
            type="button"
          >
            <RefreshCw size={12} />
            刷新
          </button>
          <div className="flex items-center gap-1.5 rounded-md border border-slate-700 bg-slate-800 px-3 py-1.5">
            <div className="h-1.5 w-1.5 rounded-full bg-green-500" />
            <span className="text-[11px] font-medium text-slate-200">{connectedCount} 台设备已配对</span>
          </div>
        </div>
      </div>

      <div className="flex flex-1 gap-4 overflow-hidden">
        <div className="flex w-[320px] shrink-0 flex-col overflow-hidden rounded-lg border border-slate-700 bg-slate-800/80 shadow-md">
          <div className="border-b border-slate-700 bg-slate-900/80 p-4">
            <h3 className="mb-1 text-sm font-semibold text-white">发送文件</h3>
            <p className="text-[11px] text-slate-400">
              目标设备：{pairedDeviceName || "暂无可用配对设备"}
            </p>
          </div>

          <div className="flex flex-1 flex-col p-4">
            <div
              className={`flex flex-1 flex-col items-center justify-center rounded-xl border border-dashed p-4 text-center transition-all ${
                isDragging
                  ? "border-blue-400 bg-blue-500/10 text-blue-300"
                  : "border-slate-700 bg-slate-900/40 text-slate-400"
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
                updatePendingFromFileList(event.dataTransfer.files);
              }}
            >
              <Upload className="mb-3 text-blue-400" size={24} />
              <div className="text-sm font-medium text-slate-200">拖拽文件到这里</div>
              <div className="mt-1 text-[11px] text-slate-500">支持一次选择多个文件，当前会保存传输验证结果到应用数据目录。</div>
            </div>

            <input
              className="hidden"
              multiple
              onChange={(event) => updatePendingFromFileList(event.target.files)}
              ref={fileInputRef}
              type="file"
            />

            <div className="mt-3 flex gap-2">
              <button
                className="flex flex-1 items-center justify-center gap-1.5 rounded-md bg-slate-700 px-3 py-2 text-xs font-medium text-slate-100 transition-colors hover:bg-slate-600"
                onClick={() => fileInputRef.current?.click()}
                type="button"
              >
                <Paperclip size={14} />
                选择文件
              </button>
              <button
                className="flex flex-1 items-center justify-center gap-1.5 rounded-md bg-blue-600 px-3 py-2 text-xs font-medium text-white transition-colors hover:bg-blue-500 disabled:bg-slate-700 disabled:text-slate-400"
                disabled={pendingFiles.length === 0}
                onClick={() => void handleSendFiles()}
                type="button"
              >
                <Send size={14} />
                发送
              </button>
            </div>

            <div className="mt-4 rounded-lg border border-slate-700/60 bg-slate-900/60 p-3">
              <div className="mb-2 flex items-center justify-between">
                <span className="text-[11px] font-medium text-slate-200">待发送文件</span>
                <span className="text-[10px] text-slate-500">{pendingFiles.length} 个</span>
              </div>
              {pendingFiles.length === 0 ? (
                <div className="text-[11px] text-slate-500">还没有选择文件。</div>
              ) : (
                <div className="space-y-2">
                  {pendingFiles.slice(0, 5).map((file) => (
                    <div className="flex items-center gap-2 rounded-md bg-slate-800/70 px-2 py-1.5" key={`${file.name}-${file.size_bytes}`}>
                      {isImageFile(file.name) ? (
                        <ImageIcon className="text-indigo-400" size={14} />
                      ) : (
                        <FileText className="text-blue-400" size={14} />
                      )}
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-[11px] text-slate-200">{file.name}</div>
                        <div className="text-[10px] text-slate-500">{formatBytes(file.size_bytes)}</div>
                      </div>
                    </div>
                  ))}
                  <div className="pt-1 text-[10px] text-slate-400">总大小 {formatBytes(totalPendingBytes)}</div>
                </div>
              )}
            </div>

            <div className="mt-3 rounded-md border border-emerald-500/20 bg-emerald-400/10 p-2 text-[10px] text-emerald-400">
              {statusText}
            </div>
            {error ? <div className="mt-2 text-[10px] text-red-400">{error}</div> : null}
          </div>
        </div>

        <div className="flex flex-1 flex-col overflow-hidden rounded-lg border border-slate-700 bg-slate-800/80 shadow-md">
          <div className="flex items-center justify-between border-b border-slate-700 bg-slate-900/70 px-4 py-3">
            <div>
              <h3 className="text-sm font-semibold text-white">最近传输记录</h3>
              <p className="text-[11px] text-slate-400">显示当前会话中已经完成的传输验证记录。</p>
            </div>
            <div className="flex items-center gap-1.5 text-[11px] text-slate-400">
              <FolderOpen size={12} />
              结果文件保存到应用数据目录 `transfers`
            </div>
          </div>

          <div className="flex-1 space-y-3 overflow-y-auto bg-slate-800/40 p-4">
            {records.length === 0 ? (
              <div className="rounded-xl border border-dashed border-slate-700 bg-slate-900/40 p-4 text-sm text-slate-400">
                还没有传输记录。选择文件并点击“发送”后，会在这里显示结果。
              </div>
            ) : null}

            {records
              .slice()
              .reverse()
              .map((record) => (
                <div className="rounded-xl border border-slate-700 bg-slate-900/60 p-4 shadow-sm" key={record.plan.manifest.transfer_id}>
                  <div className="mb-3 flex items-start justify-between gap-4">
                    <div>
                      <div className="flex items-center gap-2 text-sm font-medium text-slate-100">
                        <Monitor className="text-blue-400" size={14} />
                        <span>{record.plan.manifest.files.length} 个文件</span>
                        <span className="text-slate-500">→</span>
                        <MonitorSmartphone className="text-indigo-400" size={14} />
                      </div>
                      <div className="mt-1 text-[11px] text-slate-500">
                        传输 ID：{record.plan.manifest.transfer_id}
                      </div>
                    </div>
                    <div className="rounded-full border border-emerald-500/20 bg-emerald-400/10 px-2.5 py-1 text-[10px] font-medium text-emerald-400">
                      {formatStatus(record.progress.status)}
                    </div>
                  </div>

                  <div className="mb-3 grid grid-cols-3 gap-2 text-[11px] text-slate-400">
                    <div className="rounded-md bg-slate-800/70 px-3 py-2">
                      总大小
                      <div className="mt-1 font-mono text-slate-200">{formatBytes(record.progress.total_bytes)}</div>
                    </div>
                    <div className="rounded-md bg-slate-800/70 px-3 py-2">
                      已验证文件
                      <div className="mt-1 font-mono text-slate-200">{record.verified_files}</div>
                    </div>
                    <div className="rounded-md bg-slate-800/70 px-3 py-2">
                      耗时
                      <div className="mt-1 font-mono text-slate-200">{record.elapsed_ms} ms</div>
                    </div>
                  </div>

                  <div className="space-y-2">
                    {record.plan.manifest.files.map((file) => (
                      <div className="flex items-center gap-3 rounded-lg border border-slate-700/60 bg-slate-800/50 px-3 py-2" key={file.file_id}>
                        {isImageFile(file.name) ? (
                          <ImageIcon className="text-indigo-400" size={16} />
                        ) : (
                          <FileText className="text-blue-400" size={16} />
                        )}
                        <div className="min-w-0 flex-1">
                          <div className="truncate text-sm text-slate-200">{file.name}</div>
                          <div className="text-[10px] text-slate-500">{formatBytes(file.size_bytes)}</div>
                        </div>
                        <div className="flex items-center gap-1 text-[10px] text-slate-400">
                          <Download size={12} />
                          已生成验证文件
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              ))}
          </div>
        </div>
      </div>
    </div>
  );
}
