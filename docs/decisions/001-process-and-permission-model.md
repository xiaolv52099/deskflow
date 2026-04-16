# 001 进程与权限模型

## 背景

输入捕获、网络会话和 UI 不应耦合在同一执行路径中。Windows 和 macOS 的权限引导、常驻行为也不适合混入高频输入链。

## 决策

- MVP 采用 `app-desktop + core-service` 双进程结构
- `app-desktop` 负责 UI、托盘/菜单栏、权限引导和配对确认
- `core-service` 负责输入、会话、安全、拓扑、剪贴板
- 两者通过本地 IPC 交互
- MVP 不引入 Windows 系统服务，也不引入 macOS 登录前助手

## 结果

- 高频路径与 UI 隔离，便于稳定性和性能控制
- 权限引导可在桌面壳中集中处理
- 代价是需要额外维护本地 IPC 和进程生命周期
- Windows UAC/登录屏幕、macOS 登录前会话不纳入 MVP
