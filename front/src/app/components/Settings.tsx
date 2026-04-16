import React, { useState } from 'react';
import { MousePointer2, MoveVertical, Power, ShieldCheck, Keyboard, Terminal, Mouse, X, FolderSearch, Maximize2, Minimize2 } from 'lucide-react';

export function Settings() {
  const [pointerSpeed, setPointerSpeed] = useState(50);
  const [scrollSpeed, setScrollSpeed] = useState(60);
  const [scrollSmoothness, setScrollSmoothness] = useState(70);
  
  const [autoStart, setAutoStart] = useState(true);
  const [shareClipboard, setShareClipboard] = useState(true);
  const [encryption, setEncryption] = useState(true);

  const [logDir, setLogDir] = useState('C:\\Users\\Admin\\AppData\\Local\\KVMSync\\Logs');
  const [isLogModalOpen, setIsLogModalOpen] = useState(false);
  const [isLogMaximized, setIsLogMaximized] = useState(false);

  return (
    <div className="flex flex-col h-full animate-in fade-in slide-in-from-bottom-4 duration-500 overflow-hidden relative">
      <h2 className="text-sm font-bold mb-3 text-white tracking-wide">系统与控制设置</h2>
      
      <div className="space-y-4 overflow-y-auto pr-2 pb-6 flex-1 custom-scrollbar">
        
        {/* Peripheral Settings */}
        <div className="bg-slate-800/80 border border-slate-700/60 rounded-lg p-4 shadow-sm">
          <h3 className="text-xs font-semibold mb-3.5 flex items-center gap-1.5 text-white border-b border-slate-700 pb-2">
            <Keyboard className="text-blue-400" size={14} />
            指针与滚轮速度
          </h3>
          
          <div className="space-y-4">
            <div>
              <div className="flex justify-between mb-1.5">
                <label className="text-slate-300 text-[11px] font-medium flex items-center gap-1.5">
                  <MousePointer2 size={12} className="text-slate-400" /> 指针速度 (跨屏灵敏度)
                </label>
                <span className="text-blue-400 text-[10px] font-mono bg-slate-900 px-1.5 py-0.5 rounded">{pointerSpeed}%</span>
              </div>
              <input type="range" min="1" max="100" value={pointerSpeed} onChange={(e) => setPointerSpeed(parseInt(e.target.value))}
                className="w-full h-1 bg-slate-900 rounded-lg appearance-none cursor-pointer accent-blue-500" />
            </div>

            <div>
              <div className="flex justify-between mb-1.5">
                <label className="text-slate-300 text-[11px] font-medium flex items-center gap-1.5">
                  <Mouse size={12} className="text-slate-400" /> 滚轮速度 (一次滚动的行数)
                </label>
                <span className="text-blue-400 text-[10px] font-mono bg-slate-900 px-1.5 py-0.5 rounded">{scrollSpeed}%</span>
              </div>
              <input type="range" min="1" max="100" value={scrollSpeed} onChange={(e) => setScrollSpeed(parseInt(e.target.value))}
                className="w-full h-1 bg-slate-900 rounded-lg appearance-none cursor-pointer accent-blue-500" />
            </div>

            <div>
              <div className="flex justify-between mb-1.5">
                <label className="text-slate-300 text-[11px] font-medium flex items-center gap-1.5">
                  <MoveVertical size={12} className="text-slate-400" /> 滚轮平滑 (惯性滑动控制)
                </label>
                <span className="text-blue-400 text-[10px] font-mono bg-slate-900 px-1.5 py-0.5 rounded">{scrollSmoothness}%</span>
              </div>
              <input type="range" min="1" max="100" value={scrollSmoothness} onChange={(e) => setScrollSmoothness(parseInt(e.target.value))}
                className="w-full h-1 bg-slate-900 rounded-lg appearance-none cursor-pointer accent-blue-500" />
            </div>
          </div>
        </div>

        {/* Feature Settings */}
        <div className="bg-slate-800/80 border border-slate-700/60 rounded-lg p-4 shadow-sm">
          <h3 className="text-xs font-semibold mb-3.5 flex items-center gap-1.5 text-white border-b border-slate-700 pb-2">
            <ShieldCheck className="text-emerald-400" size={14} />
            功能与安全
          </h3>
          
          <div className="space-y-3.5">
            <div className="flex items-center justify-between">
              <div>
                <h4 className="text-slate-200 text-[11px] font-medium">全局剪贴板同步</h4>
                <p className="text-[10px] text-slate-500 mt-0.5">主副服务器间无缝复制粘贴。</p>
              </div>
              <button onClick={() => setShareClipboard(!shareClipboard)}
                className={`w-8 h-4.5 rounded-full p-0.5 transition-colors shrink-0 ${shareClipboard ? 'bg-blue-600' : 'bg-slate-600'}`}>
                <div className={`w-3.5 h-3.5 rounded-full bg-white shadow-sm transform transition-transform ${shareClipboard ? 'translate-x-3.5' : 'translate-x-0'}`} />
              </button>
            </div>

            <div className="flex items-center justify-between">
              <div>
                <h4 className="text-slate-200 text-[11px] font-medium">端到端加密通信</h4>
                <p className="text-[10px] text-slate-500 mt-0.5">AES-256 加密局域网内数据。</p>
              </div>
              <button onClick={() => setEncryption(!encryption)}
                className={`w-8 h-4.5 rounded-full p-0.5 transition-colors shrink-0 ${encryption ? 'bg-blue-600' : 'bg-slate-600'}`}>
                <div className={`w-3.5 h-3.5 rounded-full bg-white shadow-sm transform transition-transform ${encryption ? 'translate-x-3.5' : 'translate-x-0'}`} />
              </button>
            </div>
          </div>
        </div>

        {/* Logging and Advanced */}
        <div className="bg-slate-800/80 border border-slate-700/60 rounded-lg p-4 shadow-sm">
          <h3 className="text-xs font-semibold mb-3.5 flex items-center gap-1.5 text-white border-b border-slate-700 pb-2">
            <Terminal className="text-indigo-400" size={14} />
            运行日志与高级
          </h3>
          
          <div className="space-y-3.5">
            <div>
              <h4 className="text-slate-200 text-[11px] font-medium mb-1">日志目录配置</h4>
              <div className="flex gap-2 items-center">
                <div className="flex-1 bg-slate-900 border border-slate-700 rounded-md flex items-center px-2 py-1.5 h-7">
                  <FolderSearch size={12} className="text-slate-500 mr-2 shrink-0"/>
                  <input type="text" value={logDir} onChange={(e) => setLogDir(e.target.value)}
                    className="bg-transparent border-none text-[10px] text-slate-300 w-full focus:outline-none font-mono tracking-wide" />
                </div>
                <button 
                  onClick={() => setIsLogModalOpen(true)}
                  className="bg-slate-700 hover:bg-slate-600 text-slate-200 text-[10px] font-medium px-3 h-7 rounded-md transition-colors whitespace-nowrap"
                >
                  预览日志
                </button>
              </div>
            </div>

            <div className="flex items-center justify-between pt-2 border-t border-slate-700/50">
              <div>
                <h4 className="text-slate-200 text-[11px] font-medium">开机自动启动</h4>
                <p className="text-[10px] text-slate-500 mt-0.5">系统启动时自动运行并最小化到托盘。</p>
              </div>
              <button onClick={() => setAutoStart(!autoStart)}
                className={`w-8 h-4.5 rounded-full p-0.5 transition-colors shrink-0 ${autoStart ? 'bg-blue-600' : 'bg-slate-600'}`}>
                <div className={`w-3.5 h-3.5 rounded-full bg-white shadow-sm transform transition-transform ${autoStart ? 'translate-x-3.5' : 'translate-x-0'}`} />
              </button>
            </div>
          </div>
        </div>

      </div>

      {/* Log Preview Modal */}
      {isLogModalOpen && (
        <div className="absolute inset-0 z-50 flex items-center justify-center bg-slate-950/80 backdrop-blur-sm animate-in fade-in duration-200 rounded-lg">
          <div className={`bg-slate-900 border border-slate-700 rounded-lg shadow-2xl flex flex-col overflow-hidden animate-in zoom-in-95 duration-200 transition-all ${isLogMaximized ? 'w-[95%] h-[95%]' : 'w-[90%] max-w-2xl h-[70vh]'}`}>
            <div className="flex justify-between items-center px-4 py-2.5 border-b border-slate-800 bg-slate-950">
              <span className="text-[11px] font-bold text-white flex items-center gap-1.5">
                <Terminal size={12} className="text-indigo-400" />
                运行日志预览
              </span>
              <div className="flex items-center gap-3">
                <button onClick={() => setIsLogMaximized(!isLogMaximized)} className="text-slate-400 hover:text-white transition-colors">
                  {isLogMaximized ? <Minimize2 size={14} /> : <Maximize2 size={14} />}
                </button>
                <button onClick={() => setIsLogModalOpen(false)} className="text-slate-400 hover:text-white transition-colors">
                  <X size={14} />
                </button>
              </div>
            </div>
            <div className="p-3 bg-slate-950 text-[10px] font-mono text-slate-400 flex-1 overflow-y-auto leading-relaxed">
              <div className="text-indigo-400">[2023-10-27 10:24:00] INFO: KVM Sync Started v1.1.2.</div>
              <div className="text-indigo-400">[2023-10-27 10:24:01] INFO: Initializing peripheral hooks...</div>
              <div className="text-green-400">[2023-10-27 10:24:01] SUCCESS: Listening on 192.168.1.100:8080</div>
              <div className="text-slate-500">[2023-10-27 10:24:02] DEBUG: Network topology initialized.</div>
              <div className="text-indigo-400">[2023-10-27 10:25:32] INFO: Incoming connection from 192.168.1.105...</div>
              <div className="text-emerald-400">[2023-10-27 10:25:35] SUCCESS: Handshake verified. Device 'MacBook' connected.</div>
              <div className="text-yellow-400">[2023-10-27 10:30:12] WARN: Ping latency spike (45ms).</div>
              <div className="text-slate-500">[2023-10-27 10:35:00] DEBUG: Clipboard sync event triggered (File Transfer: 24.5MB).</div>
              <div className="text-slate-500">[2023-10-27 10:35:10] DEBUG: Transfer completed.</div>
              <div className="mt-2 flex items-center gap-1 text-slate-600">
                <span className="w-1.5 h-1.5 bg-slate-500 rounded-full animate-pulse"></span>
                Waiting for new events...
              </div>
            </div>
            <div className="p-2 border-t border-slate-800 bg-slate-900/50 flex justify-end gap-2">
              <button className="px-3 py-1.5 rounded-md text-[10px] text-slate-300 hover:bg-slate-800 transition-colors border border-slate-700">清空日志</button>
              <button onClick={() => setIsLogModalOpen(false)} className="px-3 py-1.5 rounded-md text-[10px] text-white bg-blue-600 hover:bg-blue-500 transition-colors">关闭</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
