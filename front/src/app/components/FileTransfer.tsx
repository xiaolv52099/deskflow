import React, { useState, useRef, useEffect } from 'react';
import { Send, Paperclip, FileText, Image as ImageIcon, Download, MonitorSmartphone, Monitor } from 'lucide-react';

interface Message {
  id: number;
  sender: 'me' | 'other';
  deviceName: string;
  type: 'text' | 'file';
  content?: string;
  fileName?: string;
  fileSize?: string;
  time: string;
}

export function FileTransfer() {
  const [messages, setMessages] = useState<Message[]>([
    { id: 1, sender: 'other', deviceName: 'MacBook Pro', type: 'text', content: '测试连接，剪贴板通吗？', time: '10:24' },
    { id: 2, sender: 'me', deviceName: 'Desktop', type: 'text', content: '可以的，支持文本和文件拖拽。', time: '10:25' },
    { id: 3, sender: 'other', deviceName: 'MacBook Pro', type: 'file', fileName: 'assets_v2.zip', fileSize: '24.5 MB', time: '10:30' },
  ]);
  const [inputText, setInputText] = useState('');
  const endOfMessagesRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    endOfMessagesRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  const handleSend = () => {
    if (!inputText.trim()) return;
    setMessages([...messages, {
      id: Date.now(),
      sender: 'me',
      deviceName: 'Desktop',
      type: 'text',
      content: inputText,
      time: new Date().toLocaleTimeString([], {hour: '2-digit', minute:'2-digit'})
    }]);
    setInputText('');
  };

  return (
    <div className="flex flex-col h-full animate-in fade-in slide-in-from-bottom-4 duration-500 w-full">
      <div className="flex items-center justify-between mb-3">
        <div>
          <h2 className="text-lg font-bold mb-0.5">文件传输助手</h2>
          <p className="text-xs text-slate-400">在局域网设备间快速发送文本和文件</p>
        </div>
        <div className="flex items-center gap-1.5 bg-slate-800 px-3 py-1.5 rounded-md border border-slate-700">
          <div className="w-1.5 h-1.5 rounded-full bg-green-500"></div>
          <span className="text-[11px] font-medium text-slate-200">2 设备已连接</span>
        </div>
      </div>
      
      <div className="flex-1 flex flex-col bg-slate-800/80 rounded-lg border border-slate-700 overflow-hidden shadow-md">
        {/* Chat area */}
        <div className="flex-1 overflow-y-auto p-4 space-y-4 bg-slate-800/40">
          {messages.map(msg => (
            <div key={msg.id} className={`flex flex-col ${msg.sender === 'me' ? 'items-end' : 'items-start'}`}>
              <div className="flex items-center gap-1.5 text-[10px] text-slate-400 mb-1 px-1">
                {msg.sender === 'other' && <MonitorSmartphone size={10} />}
                {msg.sender === 'me' && <Monitor size={10} />}
                <span>{msg.deviceName}</span>
                <span className="opacity-50">•</span>
                <span>{msg.time}</span>
              </div>
              
              {msg.type === 'text' && (
                <div className={`px-3.5 py-2 rounded-xl max-w-[80%] text-sm leading-relaxed shadow-sm ${
                  msg.sender === 'me' 
                    ? 'bg-blue-600 text-white rounded-tr-sm' 
                    : 'bg-slate-700 text-slate-100 rounded-tl-sm'
                }`}>
                  {msg.content}
                </div>
              )}

              {msg.type === 'file' && (
                <div className={`flex items-center gap-3 p-2.5 rounded-xl w-60 shadow-sm transition-all hover:bg-opacity-80 cursor-pointer ${
                  msg.sender === 'me'
                    ? 'bg-blue-900/30 border border-blue-500/30 rounded-tr-sm'
                    : 'bg-slate-700 border border-slate-600 rounded-tl-sm hover:bg-slate-600'
                }`}>
                  <div className={`p-2.5 rounded-lg ${msg.sender === 'me' ? 'bg-blue-600 shadow-blue-900/50' : 'bg-slate-800'}`}>
                    <FileText className={msg.sender === 'me' ? 'text-white' : 'text-slate-300'} size={20} />
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="font-medium text-slate-200 truncate text-sm" title={msg.fileName}>{msg.fileName}</div>
                    <div className="text-[10px] text-slate-400 mt-0.5">
                      {msg.fileSize}
                    </div>
                  </div>
                  <button className="text-slate-400 hover:text-blue-400 transition-colors p-1" title="下载文件">
                    <Download size={16} />
                  </button>
                </div>
              )}
            </div>
          ))}
          <div ref={endOfMessagesRef} />
        </div>
        
        {/* Input area */}
        <div className="bg-slate-900 border-t border-slate-700 p-3 shrink-0">
          <div className="flex gap-1.5 mb-2 px-1">
            <button className="p-1.5 text-slate-400 hover:text-white hover:bg-slate-800 rounded-md transition-colors" title="发送文件">
              <Paperclip size={16} />
            </button>
            <button className="p-1.5 text-slate-400 hover:text-white hover:bg-slate-800 rounded-md transition-colors" title="发送图片">
              <ImageIcon size={16} />
            </button>
          </div>
          
          <div className="flex gap-2 relative">
            <textarea 
              value={inputText}
              onChange={(e) => setInputText(e.target.value)}
              onKeyDown={(e) => {
                if(e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault();
                  handleSend();
                }
              }}
              placeholder="输入文本消息，或拖拽文件..."
              className="flex-1 bg-slate-800/80 border border-slate-700 rounded-md pl-3 pr-12 py-2 text-slate-100 placeholder:text-slate-500 focus:outline-none focus:border-blue-500 focus:bg-slate-800 transition-all resize-none h-10 min-h-[40px] text-sm shadow-inner"
            />
            <button 
              onClick={handleSend}
              disabled={!inputText.trim()}
              className="absolute right-1 top-1 bottom-1 bg-blue-600 hover:bg-blue-500 disabled:bg-slate-700 disabled:text-slate-500 text-white rounded px-3 flex items-center justify-center transition-colors"
            >
              <Send size={14} />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
