# Deskflow-Plus

Deskflow-Plus 是一个面向局域网场景的跨平台桌面协同项目，当前目标平台为 Windows 和 macOS。

当前仓库包含：

- Rust 核心服务与功能模块
- Tauri 2 桌面壳
- React + TypeScript 前端界面
- 需求、架构、决策、测试计划文档

当前已接入和已暴露的主要能力：

- 多机共享一套键盘和鼠标
- 局域网设备自动发现
- 手动配对与信任建立
- 3 x 3 屏幕相对位置拖拽布局
- 剪切板共享开关
- 文件传输界面与后端接线
- 鼠标灵敏度与滚轮平滑参数设置界面
- 单实例桌面应用限制

## 仓库结构

- `apps/app-desktop`
  Tauri 2 桌面应用，包含前端和桌面入口
- `apps/core-service`
  核心服务编排
- `crates/*`
  输入、会话、拓扑、文件传输、设备信任、剪切板等 Rust 功能模块
- `docs`
  需求、背景、架构、ADR、测试计划
- `task.json`
  任务状态记录

## 技术栈

- Rust
- Tauri 2
- Tokio
- Rustls
- SQLite
- React 19
- TypeScript
- Vite
- Tailwind CSS

## 环境依赖

### 通用依赖

需要先安装以下工具：

- Git
- Rust 工具链
  安装后需包含 `rustc` 和 `cargo`
- Node.js 20 或更高版本
- npm

Rust 建议使用 `rustup` 安装。

### Windows 构建环境

Windows 下建议使用：

- Windows 10 或 Windows 11
- Visual Studio 2022 Build Tools
  需要包含 C++ 构建工具和 Windows SDK
- WebView2 Runtime

说明：

- Tauri 2 在 Windows 下构建桌面产物时，依赖 MSVC 工具链和 Windows SDK
- 打包 `nsis` 安装包时，Tauri 会调用 NSIS 相关工具链

### macOS 构建环境

macOS 下建议使用：

- macOS 13 或更高版本
- Xcode Command Line Tools
- 系统可用的 WebKit / WKWebView 运行环境

安装命令：

```bash
xcode-select --install
```

说明：

- macOS 安装包必须在 macOS 机器上构建
- 如需正式分发，后续还需要 Apple Developer 签名与公证流程

## 安装依赖

### 1. 安装 Rust

Windows PowerShell：

```powershell
winget install Rustlang.Rustup
```

安装后重开终端，验证：

```powershell
rustc -V
cargo -V
```

macOS：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustc -V
cargo -V
```

### 2. 安装 Node.js

Windows：

```powershell
winget install OpenJS.NodeJS.LTS
```

macOS 可使用官网安装包或包管理器。

验证：

```bash
node -v
npm -v
```

### 3. 安装前端依赖

在 `apps/app-desktop` 目录执行：

```bash
npm install
```

## 本地开发

前端构建：

```bash
cd apps/app-desktop
npm run build
```

Rust 检查：

```bash
cargo check -p app-desktop -j 1
```

如果本机安装了 Tauri CLI，也可以继续接入本地调试流程。

## 打包命令

### Windows 安装包

在 `apps/app-desktop` 目录执行：

```powershell
cargo tauri build --bundles nsis
```

当前项目在 Windows 上已实际打包通过，产物类型为：

- NSIS 安装包 `.exe`

默认输出目录通常位于：

```text
target/release/bundle/nsis/
```

### macOS 安装包

在 macOS 机器上的 `apps/app-desktop` 目录执行：

```bash
cargo tauri build
```

Tauri 会根据当前配置生成对应的 macOS bundle，常见产物包括：

- `.app`
- `.dmg`

如需正式面向外部用户分发，还需要补充：

- Developer ID 签名
- notarization 公证

## 当前打包质量说明

### 已完成验证

以下内容已经在当前 Windows 开发机上完成过实际验证：

- `npm run build`
- `cargo check -p app-desktop -j 1`
- `cargo tauri build --bundles nsis`
- 生成 Windows 安装包
- 应用可启动到主界面
- 单实例限制已接入

### 已完成的界面集成范围

当前实际生效的前端位于：

- `apps/app-desktop/src`

此前作为参考稿存在的 `front/` 目录已经移除，不再参与构建。

### 尚未完成的发布级验证

以下内容仍需要在真实多机环境和 macOS 机器上继续补齐：

- Windows 主控到 macOS 被控的完整链路验证
- macOS 主控到 Windows 被控的完整链路验证
- 多机文件传输吞吐与稳定性验证
- 鼠标灵敏度与滚轮平滑参数的效果校准
- macOS 打包、签名、公证
- 更完整的安装/升级/卸载回归

因此，当前状态更适合作为：

- 内测包
- 开发验证包
- 功能联调用包

尚不建议直接视为最终商业发布包。

## 文档

详细文档见：

- `docs/requirements.md`
- `docs/background.md`
- `docs/architecture.md`
- `docs/test-plan.md`

## 备注

如果你要继续做发布准备，建议下一步优先补齐：

- Windows 与 macOS 双机联调回归
- macOS 侧实际打包
- 安装与卸载验证
- README 中的发布版本号、下载方式、已知问题清单
