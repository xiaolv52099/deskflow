# 002 协议与会话模型

## 背景

输入、控制、剪贴板和后续文件传输都依赖统一协议边界。若没有先定义协议，后续模块实现会重复返工。

## 决策

- 每个主控到客户端对使用一条 TLS/TCP 会话
- 在单条会话上承载版本化帧协议
- 协议消息类别划分为 `control`、`input`、`clipboard`、`diagnostic`
- 预留 `file_transfer` 类别，但不纳入 MVP 验收
- 发送优先级为 `input > control > clipboard > diagnostic > file_transfer`
- 协议不追求兼容 Deskflow 现有二进制协议

## 结果

- 会话模型简单，可先把 MVP 做稳
- 逻辑上已经为 Post-MVP 文件传输预留扩展点
- 输入流可在队列层获得明确优先级
- 后续若需要真正多流复用，再评估升级为更复杂传输模型
