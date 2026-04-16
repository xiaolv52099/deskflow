import { useEffect, useMemo, useRef, useState } from "react";
import {
  Download,
  FileText,
  Image as ImageIcon,
  Monitor,
  MonitorSmartphone,
  Paperclip,
  Send,
} from "lucide-react";
import { createTransferPlan, getDeviceManagementSnapshot, listTransferPlans, type TransferRecord } from "../lib/tauri";

interface Message {
  id: number;
  sender: "me" | "other";
  deviceName: string;
  type: "text" | "file";
  content?: string;
  fileName?: string;
  fileSize?: string;
  time: string;
}

function formatBytes(size: number) {
  if (size >= 1024 * 1024) return `${(size / 1024 / 1024).toFixed(1)} MB`;
  return `${Math.round(size / 1024)} KB`;
}

export function FileTransfer() {
  const [records, setRecords] = useState<TransferRecord[]>([]);
  const [connectedCount, setConnectedCount] = useState(0);
  const [inputText, setInputText] = useState("");
  const [messages, setMessages] = useState<Message[]>([]);
  const endOfMessagesRef = useRef<HTMLDivElement>(null);

  async function refresh() {
    const [nextRecords, devices] = await Promise.all([listTransferPlans(), getDeviceManagementSnapshot()]);
    setRecords(nextRecords);
    setConnectedCount(devices.devices.length);
  }

  useEffect(() => {
    void refresh();
  }, []);

  useEffect(() => {
    const nextMessages: Message[] = records.flatMap((record, index) => {
      const time = new Date(Date.now() - (records.length - index) * 60_000).toLocaleTimeString("zh-CN", {
        hour: "2-digit",
        minute: "2-digit",
        hour12: false,
      });
      return [
        {
          id: index * 2 + 1,
          sender: "me",
          deviceName: "Desktop",
          type: "text",
          content: `已发起 ${record.plan.manifest.files.length} 个文件的传输验证，进度 ${record.progress.status}`,
          time,
        },
        {
          id: index * 2 + 2,
          sender: "other",
          deviceName: "Remote",
          type: "file",
          fileName: record.plan.manifest.files.map((file) => file.name).join(", "),
          fileSize: formatBytes(record.plan.manifest.total_bytes),
          time,
        },
      ];
    });
    setMessages(nextMessages);
  }, [records]);

  useEffect(() => {
    endOfMessagesRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  const canSend = inputText.trim().length > 0;

  const hasRecord = useMemo(() => records.length > 0, [records]);

  async function handleSend() {
    if (!canSend) return;
    const devices = await getDeviceManagementSnapshot();
    const target = devices.devices.find((device) => device.status.toLowerCase() !== "revoked");
    if (!target) {
      setMessages((current) => [
        ...current,
        {
          id: Date.now(),
          sender: "me",
          deviceName: "Desktop",
          type: "text",
          content: "当前没有可用的已配对设备，无法发起文件传输。",
          time: new Date().toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false }),
        },
      ]);
      setInputText("");
      return;
    }

    await createTransferPlan(target.device_id, [
      { name: `${inputText.trim()}.txt`, size_bytes: 64 * 1024 },
    ]);
    setInputText("");
    await refresh();
  }

  return (
    <div className="flex h-full w-full animate-in flex-col fade-in slide-in-from-bottom-4 duration-500">
      <div className="mb-3 flex items-center justify-between">
        <div>
          <h2 className="mb-0.5 text-lg font-bold">文件传输助手</h2>
          <p className="text-xs text-slate-400">在局域网设备间快速发送文本和文件</p>
        </div>
        <div className="flex items-center gap-1.5 rounded-md border border-slate-700 bg-slate-800 px-3 py-1.5">
          <div className="h-1.5 w-1.5 rounded-full bg-green-500" />
          <span className="text-[11px] font-medium text-slate-200">{connectedCount} 设备已连接</span>
        </div>
      </div>

      <div className="flex flex-1 flex-col overflow-hidden rounded-lg border border-slate-700 bg-slate-800/80 shadow-md">
        <div className="flex-1 space-y-4 overflow-y-auto bg-slate-800/40 p-4">
          {!hasRecord ? (
            <div className="rounded-xl border border-dashed border-slate-700 bg-slate-900/40 p-4 text-sm text-slate-400">
              还没有传输记录。输入一条消息并发送后，会触发一次传输验证。
            </div>
          ) : null}

          {messages.map((msg) => (
            <div key={msg.id} className={`flex flex-col ${msg.sender === "me" ? "items-end" : "items-start"}`}>
              <div className="mb-1 flex items-center gap-1.5 px-1 text-[10px] text-slate-400">
                {msg.sender === "other" ? <MonitorSmartphone size={10} /> : <Monitor size={10} />}
                <span>{msg.deviceName}</span>
                <span className="opacity-50">·</span>
                <span>{msg.time}</span>
              </div>

              {msg.type === "text" ? (
                <div
                  className={`max-w-[80%] rounded-xl px-3.5 py-2 text-sm leading-relaxed shadow-sm ${
                    msg.sender === "me"
                      ? "rounded-tr-sm bg-blue-600 text-white"
                      : "rounded-tl-sm bg-slate-700 text-slate-100"
                  }`}
                >
                  {msg.content}
                </div>
              ) : (
                <div
                  className={`flex w-60 cursor-pointer items-center gap-3 rounded-xl border p-2.5 shadow-sm transition-all hover:bg-opacity-80 ${
                    msg.sender === "me"
                      ? "rounded-tr-sm border-blue-500/30 bg-blue-900/30"
                      : "rounded-tl-sm border-slate-600 bg-slate-700 hover:bg-slate-600"
                  }`}
                >
                  <div className={`rounded-lg p-2.5 ${msg.sender === "me" ? "bg-blue-600 shadow-blue-900/50" : "bg-slate-800"}`}>
                    <FileText className={msg.sender === "me" ? "text-white" : "text-slate-300"} size={20} />
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium text-slate-200" title={msg.fileName}>
                      {msg.fileName}
                    </div>
                    <div className="mt-0.5 text-[10px] text-slate-400">{msg.fileSize}</div>
                  </div>
                  <button className="p-1 text-slate-400 transition-colors hover:text-blue-400" title="查看记录" type="button">
                    <Download size={16} />
                  </button>
                </div>
              )}
            </div>
          ))}
          <div ref={endOfMessagesRef} />
        </div>

        <div className="shrink-0 border-t border-slate-700 bg-slate-900 p-3">
          <div className="mb-2 flex gap-1.5 px-1">
            <button className="rounded-md p-1.5 text-slate-400 transition-colors hover:bg-slate-800 hover:text-white" title="发送文件" type="button">
              <Paperclip size={16} />
            </button>
            <button className="rounded-md p-1.5 text-slate-400 transition-colors hover:bg-slate-800 hover:text-white" title="发送图片" type="button">
              <ImageIcon size={16} />
            </button>
          </div>

          <div className="relative flex gap-2">
            <textarea
              className="h-10 min-h-[40px] flex-1 resize-none rounded-md border border-slate-700 bg-slate-800/80 py-2 pl-3 pr-12 text-sm text-slate-100 shadow-inner transition-all placeholder:text-slate-500 focus:border-blue-500 focus:bg-slate-800 focus:outline-none"
              onChange={(event) => setInputText(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  void handleSend();
                }
              }}
              placeholder="输入文本消息，或拖拽文件..."
              value={inputText}
            />
            <button
              className="absolute bottom-1 right-1 top-1 flex items-center justify-center rounded bg-blue-600 px-3 text-white transition-colors hover:bg-blue-500 disabled:bg-slate-700 disabled:text-slate-500"
              disabled={!canSend}
              onClick={() => void handleSend()}
              type="button"
            >
              <Send size={14} />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
