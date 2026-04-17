import { FolderSync, Network, Settings as SettingsIcon } from "lucide-react";

interface SidebarProps {
  activeTab: string;
  setActiveTab: (tab: string) => void;
}

export function Sidebar({ activeTab, setActiveTab }: SidebarProps) {
  const tabs = [
    { id: "connection", label: "连接配置", icon: Network },
    { id: "files", label: "文件传输", icon: FolderSync },
    { id: "settings", label: "系统设置", icon: SettingsIcon },
  ];

  return (
    <div className="flex w-36 shrink-0 select-none flex-col border-r border-slate-700 bg-slate-800">
      <nav className="flex-1 space-y-1 px-2.5 py-4">
        {tabs.map((tab) => {
          const Icon = tab.icon;
          const isActive = activeTab === tab.id;
          return (
            <button
              key={tab.id}
              className={`flex w-full flex-col items-center justify-center gap-1.5 rounded-xl border px-2 py-4 text-xs transition-all ${
                isActive
                  ? "border-blue-500/30 bg-blue-600/20 text-blue-400 shadow-sm"
                  : "border-transparent text-slate-400 hover:bg-slate-700/50 hover:text-slate-200"
              }`}
              onClick={() => setActiveTab(tab.id)}
              type="button"
            >
              <Icon className={isActive ? "text-blue-400" : "text-slate-400"} size={22} />
              <span className="font-medium">{tab.label}</span>
            </button>
          );
        })}
      </nav>
      <div className="border-t border-slate-700/60 p-3 text-center font-mono text-[10px] text-slate-500">v0.1.0</div>
    </div>
  );
}
