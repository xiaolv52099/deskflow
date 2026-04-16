import React from 'react';
import { Network, FolderSync, Settings as SettingsIcon, Monitor } from 'lucide-react';

interface SidebarProps {
  activeTab: string;
  setActiveTab: (tab: string) => void;
}

export function Sidebar({ activeTab, setActiveTab }: SidebarProps) {
  const tabs = [
    { id: 'connection', label: '连接配置', icon: Network },
    { id: 'files', label: '文件传输', icon: FolderSync },
    { id: 'settings', label: '系统设置', icon: SettingsIcon },
  ];

  return (
    <div className="w-36 bg-slate-800 flex flex-col border-r border-slate-700 shrink-0 select-none">
      <nav className="flex-1 px-2.5 py-4 space-y-1">
        {tabs.map(tab => {
          const Icon = tab.icon;
          const isActive = activeTab === tab.id;
          return (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`w-full flex flex-col items-center justify-center gap-1.5 px-2 py-4 rounded-xl transition-all text-xs ${
                isActive 
                  ? 'bg-blue-600/20 text-blue-400 shadow-sm border border-blue-500/30' 
                  : 'text-slate-400 hover:bg-slate-700/50 hover:text-slate-200 border border-transparent'
              }`}
            >
              <Icon size={22} className={isActive ? "text-blue-400" : "text-slate-400"} />
              <span className="font-medium">{tab.label}</span>
            </button>
          );
        })}
      </nav>
      <div className="p-3 text-[10px] text-slate-500 text-center border-t border-slate-700/60 font-mono">
        v1.1.2
      </div>
    </div>
  );
}
