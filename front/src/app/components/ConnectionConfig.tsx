import React, { useState } from 'react';
import { useDrag, useDrop } from 'react-dnd';
import { Copy, RefreshCw, CheckCircle2, Monitor, MonitorSmartphone, Server, MousePointer2, MoveVertical, Mouse, Terminal, X, FolderSearch, Edit2, Check } from 'lucide-react';

const ItemType = 'SCREEN';

interface ScreenProps {
  id: string;
  name: string;
  isMaster: boolean;
  position: number;
}

const ScreenBox = ({ id, name, isMaster, position }: ScreenProps) => {
  const [{ isDragging }, drag] = useDrag(() => ({
    type: ItemType,
    item: { id, position },
    collect: (monitor) => ({
      isDragging: !!monitor.isDragging(),
    }),
  }));

  return (
    <div
      ref={(node) => { drag(node); }}
      className={`flex flex-col items-center justify-center p-1 rounded-lg shadow-sm cursor-grab active:cursor-grabbing border transition-all h-full w-full select-none ${
        isMaster 
          ? 'bg-blue-900/60 border-blue-500 text-blue-100 shadow-blue-900/20' 
          : 'bg-slate-800 border-slate-600 hover:border-slate-500 text-slate-200'
      } ${isDragging ? 'opacity-40 scale-95' : 'opacity-100 hover:scale-[1.02]'}`}
    >
      {isMaster ? <Monitor size={20} className="mb-1 text-blue-400" /> : <MonitorSmartphone size={16} className="mb-1 text-slate-400" />}
      <span className="font-medium text-[10px] truncate w-full text-center tracking-wide">{name}</span>
    </div>
  );
};

const GridCell = ({ position, screen, onMoveScreen }: { position: number, screen: ScreenProps | undefined, onMoveScreen: any }) => {
  const [{ isOver }, drop] = useDrop(() => ({
    accept: ItemType,
    drop: (item: { id: string, position: number }) => onMoveScreen(item.id, position),
    collect: (monitor) => ({
      isOver: !!monitor.isOver(),
    }),
  }));

  return (
    <div 
      ref={(node) => { drop(node); }}
      className={`h-[68px] w-[68px] rounded-xl border border-dashed transition-all flex items-center justify-center p-1 ${
        isOver 
          ? 'border-blue-400 bg-blue-500/10 scale-105 border-solid shadow-inner' 
          : 'border-slate-700/60 bg-slate-900/40 hover:bg-slate-800/60'
      }`}
    >
      {screen ? (
        <ScreenBox id={screen.id} name={screen.name} isMaster={screen.isMaster} position={position} />
      ) : (
        <span className="text-slate-700 font-mono text-sm font-bold opacity-30">{position + 1}</span>
      )}
    </div>
  );
};

export function ConnectionConfig() {
  const [isMaster, setIsMaster] = useState(true);
  const [key, setKey] = useState('A7B9-C3D4-E5F6-G7H8');
  const [copied, setCopied] = useState<string | null>(null);
  const [machineName, setMachineName] = useState('My-PC');
  const [isEditingName, setIsEditingName] = useState(false);

  const [screens, setScreens] = useState<ScreenProps[]>([
    { id: 'master-1', name: 'Desktop', isMaster: true, position: 4 },
    { id: 'sub-1', name: 'MacBook', isMaster: false, position: 5 },
  ]);

  const handleCopy = (text: string, type: string) => {
    navigator.clipboard.writeText(text);
    setCopied(type);
    setTimeout(() => setCopied(null), 2000);
  };

  const generateKey = () => {
    const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789';
    let newKey = '';
    for(let i=0; i<4; i++) {
      let chunk = '';
      for(let j=0; j<4; j++) {
        chunk += chars.charAt(Math.floor(Math.random() * chars.length));
      }
      newKey += chunk + (i < 3 ? '-' : '');
    }
    setKey(newKey);
  };

  const handleMoveScreen = (id: string, newPosition: number) => {
    setScreens(prev => {
      const screenIndex = prev.findIndex(s => s.id === id);
      const targetIndex = prev.findIndex(s => s.position === newPosition);
      
      const newScreens = [...prev];
      if (targetIndex !== -1) {
        newScreens[targetIndex].position = prev[screenIndex].position;
      }
      newScreens[screenIndex].position = newPosition;
      return newScreens;
    });
  };

  return (
    <div className="flex h-full gap-4 animate-in fade-in slide-in-from-bottom-4 duration-500 w-full">
      
      {/* Left Column: Server Config */}
      <div className="flex-1 flex flex-col bg-slate-800/60 rounded-xl border border-slate-700/60 p-4 shadow-inner overflow-y-auto">
        <div className="flex bg-slate-900 p-1 rounded-lg shadow-sm border border-slate-700 mb-5 shrink-0">
          <button 
            className={`flex-1 py-1.5 rounded-md text-xs font-medium transition-all ${isMaster ? 'bg-blue-600 text-white shadow' : 'text-slate-400 hover:text-white'}`}
            onClick={() => setIsMaster(true)}
          >
            主控端
          </button>
          <button 
            className={`flex-1 py-1.5 rounded-md text-xs font-medium transition-all ${!isMaster ? 'bg-blue-600 text-white shadow' : 'text-slate-400 hover:text-white'}`}
            onClick={() => setIsMaster(false)}
          >
            被控端
          </button>
        </div>

        {isMaster ? (
          <div className="space-y-3 flex-1 flex flex-col justify-center">
            <div>
              <h3 className="text-xs font-semibold text-white mb-2 flex items-center gap-1.5">
                <Server size={14} className="text-blue-400"/> 本机网络信息
              </h3>
              
              <div className="space-y-1.5">
                <div className="bg-slate-900/80 p-2 rounded-lg border border-slate-700/60 flex justify-between items-center">
                  <div className="flex-1 mr-2">
                    <span className="text-slate-500 text-[10px] block mb-0.5">本机名称</span>
                    {isEditingName ? (
                      <div className="flex items-center gap-2">
                        <input
                          type="text"
                          value={machineName}
                          onChange={(e) => setMachineName(e.target.value)}
                          className="bg-slate-800 border border-slate-600 rounded px-1.5 py-0.5 text-xs text-white font-mono w-full focus:outline-none focus:border-blue-500"
                          autoFocus
                          onKeyDown={(e) => e.key === 'Enter' && setIsEditingName(false)}
                        />
                        <button onClick={() => setIsEditingName(false)} className="text-green-500 hover:text-green-400">
                          <Check size={14} />
                        </button>
                      </div>
                    ) : (
                      <div className="flex items-center justify-between group">
                        <span className="font-mono text-xs text-blue-400">{machineName}</span>
                        <button onClick={() => setIsEditingName(true)} className="opacity-0 group-hover:opacity-100 text-slate-400 hover:text-white transition-opacity">
                          <Edit2 size={12} />
                        </button>
                      </div>
                    )}
                  </div>
                </div>

                <div className="bg-slate-900/80 p-2 rounded-lg border border-slate-700/60 flex justify-between items-center">
                  <div>
                    <span className="text-slate-500 text-[10px] block mb-0.5">局域网 IP</span>
                    <span className="font-mono text-xs text-blue-400">192.168.1.100</span>
                  </div>
                  <button onClick={() => handleCopy('192.168.1.100', 'ip')} className="p-1.5 bg-slate-800 rounded-md text-slate-400 hover:text-white hover:bg-slate-700 transition-colors" title="复制 IP">
                    {copied === 'ip' ? <CheckCircle2 size={14} className="text-green-500" /> : <Copy size={14}/>}
                  </button>
                </div>
                
                <div className="bg-slate-900/80 p-2 rounded-lg border border-slate-700/60 flex justify-between items-center">
                  <div>
                    <span className="text-slate-500 text-[10px] block mb-0.5">服务端口</span>
                    <span className="font-mono text-xs text-blue-400">20480</span>
                  </div>
                  <button onClick={() => handleCopy('20480', 'port')} className="p-1.5 bg-slate-800 rounded-md text-slate-400 hover:text-white hover:bg-slate-700 transition-colors" title="复制端口">
                    {copied === 'port' ? <CheckCircle2 size={14} className="text-green-500" /> : <Copy size={14}/>}
                  </button>
                </div>
              </div>
            </div>

            <div className="pt-1">
              <h3 className="text-xs font-semibold text-white mb-2">动态配对密钥</h3>
              <div className="bg-slate-900/80 p-2.5 rounded-lg border border-slate-700/60 flex justify-between items-center">
                <span className="font-mono text-sm tracking-widest text-emerald-400 font-bold">{key}</span>
                <div className="flex gap-1.5">
                  <button onClick={generateKey} className="p-1.5 bg-slate-800 rounded-md text-slate-400 hover:text-white hover:bg-slate-700 transition-colors" title="刷新密钥">
                    <RefreshCw size={14}/>
                  </button>
                  <button onClick={() => handleCopy(key, 'key')} className="p-1.5 bg-slate-800 rounded-md text-slate-400 hover:text-white hover:bg-slate-700 transition-colors" title="复制密钥">
                    {copied === 'key' ? <CheckCircle2 size={14} className="text-emerald-500" /> : <Copy size={14}/>}
                  </button>
                </div>
              </div>
            </div>
            
            <div className="mt-auto flex items-center justify-center gap-2 text-[10px] text-emerald-400 bg-emerald-400/10 p-2 rounded-md border border-emerald-500/20">
              <div className="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse"></div>
              运行中，监听 192.168.1.100:20480
            </div>
          </div>
        ) : (
          <div className="space-y-4 flex-1 flex flex-col justify-center">
            <div>
              <h3 className="text-xs font-semibold text-white mb-3 flex items-center gap-1.5">
                <Server size={14} className="text-blue-400"/> 连接到主服务器
              </h3>
            </div>
            
            <div className="space-y-3">
              <div className="grid grid-cols-3 gap-2">
                <div className="col-span-2 space-y-1">
                  <label className="text-[10px] text-slate-400">主控端 IP</label>
                  <input type="text" placeholder="192.168.1.100" className="w-full bg-slate-900 border border-slate-700 rounded-md px-2.5 py-1.5 text-xs text-white focus:outline-none focus:border-blue-500 transition-all font-mono" />
                </div>
                <div className="space-y-1">
                  <label className="text-[10px] text-slate-400">端口</label>
                  <input type="text" placeholder="20480" className="w-full bg-slate-900 border border-slate-700 rounded-md px-2.5 py-1.5 text-xs text-white focus:outline-none focus:border-blue-500 transition-all font-mono" defaultValue="20480" />
                </div>
              </div>
              
              <div className="space-y-1">
                <label className="text-[10px] text-slate-400">配对密钥</label>
                <input type="text" placeholder="XXXX-XXXX-XXXX-XXXX" className="w-full bg-slate-900 border border-slate-700 rounded-md px-2.5 py-1.5 text-xs text-emerald-400 focus:outline-none focus:border-blue-500 transition-all font-mono uppercase tracking-widest font-bold" />
              </div>
            </div>

            <button className="w-full mt-auto bg-blue-600 hover:bg-blue-500 text-white font-medium py-2 px-4 rounded-md transition-colors shadow-sm text-xs">
              连接主控端
            </button>
          </div>
        )}
      </div>

      {/* Right Column: Topology */}
      {isMaster ? (
        <div className="flex-1 bg-slate-800/60 rounded-xl border border-slate-700/60 p-4 shadow-inner flex flex-col relative overflow-hidden">
          <h3 className="text-xs font-semibold text-white mb-1">屏幕拓扑结构</h3>
          <p className="text-[10px] text-slate-400 mb-4">拖拽屏幕以调整相对物理布局跨越边界。</p>

          <div className="flex-1 flex flex-col items-center justify-center">
            <div className="relative">
              {/* Visual indicators */}
              <div className="absolute -top-4 left-1/2 transform -translate-x-1/2 text-slate-600 text-[9px]">上方边界</div>
              <div className="absolute -bottom-4 left-1/2 transform -translate-x-1/2 text-slate-600 text-[9px]">下方边界</div>
              <div className="absolute top-1/2 -left-7 transform -translate-y-1/2 -rotate-90 text-slate-600 text-[9px] whitespace-nowrap">左侧边界</div>
              <div className="absolute top-1/2 -right-7 transform -translate-y-1/2 rotate-90 text-slate-600 text-[9px] whitespace-nowrap">右侧边界</div>

              <div className="grid grid-cols-3 gap-2 bg-slate-900/30 p-3 rounded-2xl border border-slate-700/30">
                {[0, 1, 2, 3, 4, 5, 6, 7, 8].map(pos => {
                  const screen = screens.find(s => s.position === pos);
                  return (
                    <GridCell 
                      key={pos} 
                      position={pos} 
                      screen={screen} 
                      onMoveScreen={handleMoveScreen} 
                    />
                  );
                })}
              </div>
            </div>
          </div>
        </div>
      ) : (
        <div className="flex-1 hidden md:block opacity-0 pointer-events-none"></div>
      )}

    </div>
  );
}
