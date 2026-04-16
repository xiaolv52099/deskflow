import { useState } from "react";
import { DndProvider } from "react-dnd";
import { HTML5Backend } from "react-dnd-html5-backend";
import { Sidebar } from "./components/Sidebar";
import { ConnectionConfig } from "./components/ConnectionConfig";
import { FileTransfer } from "./components/FileTransfer";
import { Settings } from "./components/Settings";

export default function App() {
  const [activeTab, setActiveTab] = useState("connection");

  return (
    <DndProvider backend={HTML5Backend}>
      <div className="relative flex h-screen w-full overflow-hidden rounded-xl border border-slate-700/60 bg-slate-900 font-sans text-slate-100 shadow-2xl shadow-blue-900/10 selection:bg-blue-500/30">
        <Sidebar activeTab={activeTab} setActiveTab={setActiveTab} />
        <main className="relative flex flex-1 flex-col overflow-hidden bg-[radial-gradient(ellipse_at_top,_var(--tw-gradient-stops))] from-slate-800 via-slate-900 to-slate-950 p-4">
          <div className="pointer-events-none absolute right-0 top-0 -mr-20 -mt-20 h-72 w-72 rounded-full bg-blue-500/5 blur-3xl" />
          <div className="pointer-events-none absolute bottom-0 left-48 h-96 w-96 rounded-full bg-indigo-500/5 blur-3xl" />
          <div className="relative z-10 flex h-full w-full flex-col">
            {activeTab === "connection" && <ConnectionConfig />}
            {activeTab === "files" && <FileTransfer />}
            {activeTab === "settings" && <Settings />}
          </div>
        </main>
      </div>
    </DndProvider>
  );
}
