import React, { useState } from 'react';
import { Sidebar } from './components/Sidebar';
import { ConnectionConfig } from './components/ConnectionConfig';
import { FileTransfer } from './components/FileTransfer';
import { Settings } from './components/Settings';
import { DndProvider } from 'react-dnd';
import { HTML5Backend } from 'react-dnd-html5-backend';
import { Minus, Square, X, Monitor } from 'lucide-react';

export default function App() {
  const [activeTab, setActiveTab] = useState('connection');

  return (
    <DndProvider backend={HTML5Backend}>
      <div className="flex items-center justify-center h-screen w-full bg-slate-950 text-slate-100 overflow-hidden font-sans selection:bg-blue-500/30 p-4">
        
        {/* Simulated Desktop Window - Compact Mode */}
        <div className="flex flex-col w-[760px] h-[520px] bg-slate-900 rounded-xl shadow-2xl shadow-blue-900/10 border border-slate-700/60 overflow-hidden relative">
          
          {/* Custom Window Title Bar */}
          <div className="h-9 w-full bg-slate-950 border-b border-slate-800 flex items-center justify-between px-3 select-none shrink-0 cursor-default rounded-t-xl z-20">
            <div className="flex items-center gap-2 pl-1 text-slate-400">
              <Monitor className="text-blue-500 drop-shadow-sm" size={14} />
              <span className="text-[11px] font-semibold text-slate-300 tracking-wider">KVM Sync</span>
            </div>
            <div className="flex items-center gap-3 pr-1">
              <button className="text-slate-500 hover:text-white cursor-pointer transition-colors outline-none" title="最小化">
                <Minus size={14} strokeWidth={2.5} />
              </button>
              <button className="text-slate-500 hover:text-white cursor-pointer transition-colors outline-none" title="最大化">
                <Square size={12} strokeWidth={2.5} />
              </button>
              <button className="text-slate-500 hover:text-red-500 cursor-pointer transition-colors outline-none" title="关闭">
                <X size={15} strokeWidth={2.5} />
              </button>
            </div>
          </div>

          <div className="flex flex-1 overflow-hidden relative">
            <Sidebar activeTab={activeTab} setActiveTab={setActiveTab} />
            
            <main className="flex-1 flex flex-col p-4 overflow-hidden bg-[radial-gradient(ellipse_at_top,_var(--tw-gradient-stops))] from-slate-800 via-slate-900 to-slate-950 relative">
              
              {/* Subtle background decoration */}
              <div className="absolute top-0 right-0 -mr-20 -mt-20 w-72 h-72 bg-blue-500/5 rounded-full blur-3xl pointer-events-none"></div>
              <div className="absolute bottom-0 left-48 w-96 h-96 bg-indigo-500/5 rounded-full blur-3xl pointer-events-none"></div>

              <div className="relative z-10 w-full h-full flex flex-col">
                {activeTab === 'connection' && <ConnectionConfig />}
                {activeTab === 'files' && <FileTransfer />}
                {activeTab === 'settings' && <Settings />}
              </div>
            </main>
          </div>

        </div>

      </div>
    </DndProvider>
  );
}
