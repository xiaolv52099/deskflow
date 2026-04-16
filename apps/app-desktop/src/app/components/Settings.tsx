import { useEffect, useState } from "react";
import {
  FolderSearch,
  Keyboard,
  Maximize2,
  Minimize2,
  Mouse,
  MousePointer2,
  MoveVertical,
  ShieldCheck,
  Terminal,
  X,
} from "lucide-react";
import {
  getClipboardState,
  getLogPreview,
  getRuntimeOverview,
  getInputTuning,
  setClipboardEnabled,
  updateInputTuning,
} from "../lib/tauri";

function toPercent(value: number, max = 3) {
  return Math.round((value / max) * 100);
}

function fromPercent(value: number, max = 3) {
  return (value / 100) * max;
}

export function Settings() {
  const [pointerSpeed, setPointerSpeed] = useState(50);
  const [scrollSpeed, setScrollSpeed] = useState(60);
  const [scrollSmoothness, setScrollSmoothness] = useState(70);
  const [shareClipboard, setShareClipboard] = useState(true);
  const [encryption, setEncryption] = useState(true);
  const [logDir, setLogDir] = useState("");
  const [isLogModalOpen, setIsLogModalOpen] = useState(false);
  const [isLogMaximized, setIsLogMaximized] = useState(false);
  const [logLines, setLogLines] = useState<string[]>([]);

  useEffect(() => {
    void (async () => {
      const [tuning, clipboard, runtime, logPreview] = await Promise.all([
        getInputTuning(),
        getClipboardState(),
        getRuntimeOverview(),
        getLogPreview(),
      ]);
      setPointerSpeed(toPercent(tuning.pointer_speed_multiplier));
      setScrollSpeed(toPercent(tuning.wheel_speed_multiplier));
      setScrollSmoothness(Math.round(tuning.wheel_smoothing_factor * 100));
      setShareClipboard(clipboard.enabled);
      setEncryption(runtime.health.auto_discovery_enabled);
      setLogDir(logPreview.log_path);
      setLogLines(logPreview.lines);
    })();
  }, []);

  async function persistTuning(nextPointer = pointerSpeed, nextScroll = scrollSpeed, nextSmooth = scrollSmoothness) {
    await updateInputTuning({
      pointer_speed_multiplier: fromPercent(nextPointer),
      wheel_speed_multiplier: fromPercent(nextScroll),
      wheel_smoothing_factor: nextSmooth / 100,
    });
  }

  async function handleClipboardToggle() {
    const next = !shareClipboard;
    await setClipboardEnabled(next);
    setShareClipboard(next);
  }

  async function handleOpenLogs() {
    const preview = await getLogPreview();
    setLogDir(preview.log_path);
    setLogLines(preview.lines);
    setIsLogModalOpen(true);
  }

  return (
    <div className="relative flex h-full flex-col overflow-hidden animate-in fade-in slide-in-from-bottom-4 duration-500">
      <h2 className="mb-3 text-sm font-bold tracking-wide text-white">系统与控制设置</h2>

      <div className="custom-scrollbar flex-1 space-y-4 overflow-y-auto pr-2 pb-6">
        <div className="rounded-lg border border-slate-700/60 bg-slate-800/80 p-4 shadow-sm">
          <h3 className="mb-3.5 flex items-center gap-1.5 border-b border-slate-700 pb-2 text-xs font-semibold text-white">
            <Keyboard className="text-blue-400" size={14} />
            指针与滚轮速度
          </h3>

          <div className="space-y-4">
            <div>
              <div className="mb-1.5 flex justify-between">
                <label className="flex items-center gap-1.5 text-[11px] font-medium text-slate-300">
                  <MousePointer2 className="text-slate-400" size={12} /> 指针速度
                </label>
                <span className="rounded bg-slate-900 px-1.5 py-0.5 font-mono text-[10px] text-blue-400">{pointerSpeed}%</span>
              </div>
              <input
                className="h-1 w-full cursor-pointer appearance-none rounded-lg bg-slate-900 accent-blue-500"
                max="100"
                min="1"
                onChange={(event) => {
                  const next = Number(event.target.value);
                  setPointerSpeed(next);
                  void persistTuning(next, scrollSpeed, scrollSmoothness);
                }}
                type="range"
                value={pointerSpeed}
              />
            </div>

            <div>
              <div className="mb-1.5 flex justify-between">
                <label className="flex items-center gap-1.5 text-[11px] font-medium text-slate-300">
                  <Mouse className="text-slate-400" size={12} /> 滚轮速度
                </label>
                <span className="rounded bg-slate-900 px-1.5 py-0.5 font-mono text-[10px] text-blue-400">{scrollSpeed}%</span>
              </div>
              <input
                className="h-1 w-full cursor-pointer appearance-none rounded-lg bg-slate-900 accent-blue-500"
                max="100"
                min="1"
                onChange={(event) => {
                  const next = Number(event.target.value);
                  setScrollSpeed(next);
                  void persistTuning(pointerSpeed, next, scrollSmoothness);
                }}
                type="range"
                value={scrollSpeed}
              />
            </div>

            <div>
              <div className="mb-1.5 flex justify-between">
                <label className="flex items-center gap-1.5 text-[11px] font-medium text-slate-300">
                  <MoveVertical className="text-slate-400" size={12} /> 滚轮平滑
                </label>
                <span className="rounded bg-slate-900 px-1.5 py-0.5 font-mono text-[10px] text-blue-400">{scrollSmoothness}%</span>
              </div>
              <input
                className="h-1 w-full cursor-pointer appearance-none rounded-lg bg-slate-900 accent-blue-500"
                max="100"
                min="1"
                onChange={(event) => {
                  const next = Number(event.target.value);
                  setScrollSmoothness(next);
                  void persistTuning(pointerSpeed, scrollSpeed, next);
                }}
                type="range"
                value={scrollSmoothness}
              />
            </div>
          </div>
        </div>

        <div className="rounded-lg border border-slate-700/60 bg-slate-800/80 p-4 shadow-sm">
          <h3 className="mb-3.5 flex items-center gap-1.5 border-b border-slate-700 pb-2 text-xs font-semibold text-white">
            <ShieldCheck className="text-emerald-400" size={14} />
            功能与安全
          </h3>

          <div className="space-y-3.5">
            <div className="flex items-center justify-between">
              <div>
                <h4 className="text-[11px] font-medium text-slate-200">全局剪贴板同步</h4>
                <p className="mt-0.5 text-[10px] text-slate-500">主副设备间共享剪贴板状态。</p>
              </div>
              <button
                className={`h-4.5 w-8 shrink-0 rounded-full p-0.5 transition-colors ${shareClipboard ? "bg-blue-600" : "bg-slate-600"}`}
                onClick={() => void handleClipboardToggle()}
                type="button"
              >
                <div className={`h-3.5 w-3.5 rounded-full bg-white shadow-sm transition-transform ${shareClipboard ? "translate-x-3.5" : "translate-x-0"}`} />
              </button>
            </div>

            <div className="flex items-center justify-between">
              <div>
                <h4 className="text-[11px] font-medium text-slate-200">端到端加密通信</h4>
                <p className="mt-0.5 text-[10px] text-slate-500">TLS 信任配对和局域网发现链路已开启。</p>
              </div>
              <button
                className={`h-4.5 w-8 shrink-0 rounded-full p-0.5 transition-colors ${encryption ? "bg-blue-600" : "bg-slate-600"}`}
                onClick={() => setEncryption((value) => !value)}
                type="button"
              >
                <div className={`h-3.5 w-3.5 rounded-full bg-white shadow-sm transition-transform ${encryption ? "translate-x-3.5" : "translate-x-0"}`} />
              </button>
            </div>
          </div>
        </div>

        <div className="rounded-lg border border-slate-700/60 bg-slate-800/80 p-4 shadow-sm">
          <h3 className="mb-3.5 flex items-center gap-1.5 border-b border-slate-700 pb-2 text-xs font-semibold text-white">
            <Terminal className="text-indigo-400" size={14} />
            运行日志与高级
          </h3>

          <div className="space-y-3.5">
            <div>
              <h4 className="mb-1 text-[11px] font-medium text-slate-200">日志目录配置</h4>
              <div className="flex items-center gap-2">
                <div className="flex h-7 flex-1 items-center rounded-md border border-slate-700 bg-slate-900 px-2 py-1.5">
                  <FolderSearch className="mr-2 shrink-0 text-slate-500" size={12} />
                  <input
                    className="w-full border-none bg-transparent font-mono text-[10px] tracking-wide text-slate-300 focus:outline-none"
                    onChange={(event) => setLogDir(event.target.value)}
                    value={logDir}
                  />
                </div>
                <button
                  className="h-7 whitespace-nowrap rounded-md bg-slate-700 px-3 text-[10px] font-medium text-slate-200 transition-colors hover:bg-slate-600"
                  onClick={() => void handleOpenLogs()}
                  type="button"
                >
                  预览日志
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>

      {isLogModalOpen ? (
        <div className="absolute inset-0 z-50 flex items-center justify-center rounded-lg bg-slate-950/80 backdrop-blur-sm animate-in fade-in duration-200">
          <div
            className={`flex flex-col overflow-hidden rounded-lg border border-slate-700 bg-slate-900 shadow-2xl transition-all animate-in zoom-in-95 duration-200 ${
              isLogMaximized ? "h-[95%] w-[95%]" : "h-[70vh] w-[90%] max-w-2xl"
            }`}
          >
            <div className="flex items-center justify-between border-b border-slate-800 bg-slate-950 px-4 py-2.5">
              <span className="flex items-center gap-1.5 text-[11px] font-bold text-white">
                <Terminal className="text-indigo-400" size={12} />
                运行日志预览
              </span>
              <div className="flex items-center gap-3">
                <button className="text-slate-400 transition-colors hover:text-white" onClick={() => setIsLogMaximized((value) => !value)} type="button">
                  {isLogMaximized ? <Minimize2 size={14} /> : <Maximize2 size={14} />}
                </button>
                <button className="text-slate-400 transition-colors hover:text-white" onClick={() => setIsLogModalOpen(false)} type="button">
                  <X size={14} />
                </button>
              </div>
            </div>
            <div className="flex-1 overflow-y-auto bg-slate-950 p-3 font-mono text-[10px] leading-relaxed text-slate-400">
              {logLines.length === 0 ? <div>暂无日志输出。</div> : logLines.map((line, index) => <div key={`${line}-${index}`}>{line}</div>)}
              <div className="mt-2 flex items-center gap-1 text-slate-600">
                <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-slate-500" />
                Waiting for new events...
              </div>
            </div>
            <div className="flex justify-end gap-2 border-t border-slate-800 bg-slate-900/50 p-2">
              <button className="rounded-md border border-slate-700 px-3 py-1.5 text-[10px] text-slate-300 transition-colors hover:bg-slate-800" onClick={() => setLogLines([])} type="button">
                清空预览
              </button>
              <button className="rounded-md bg-blue-600 px-3 py-1.5 text-[10px] text-white transition-colors hover:bg-blue-500" onClick={() => setIsLogModalOpen(false)} type="button">
                关闭
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
