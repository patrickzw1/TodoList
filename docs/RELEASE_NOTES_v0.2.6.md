# TodoList v0.2.6

本次更新包含“今日”视图的日期修复，并修正首次 GitHub Actions 自动发布流程中发现的两个问题。

## 修复与兼容性

- “今日”只显示截止日期有效且不晚于本机当天的未归档任务。空日期、未安排日期、无效旧日期、未来日期和归档任务不会再误入。
- “未安排”和旧版无效日期仍保留在“全部”及对应项目中；升级不会补写、迁移或删除用户现存任务数据。
- MCP 创建任务时省略或传空日期都会保存为“未安排”；更新任务时传空日期会清除日期，不传日期字段则保留原值。非空无效日期会被原子拒绝。
- CI 会在 Rust 桌面测试前准备真实的 development MCP sidecar，避免干净 Windows runner 因缺少 Tauri external binary 而失败。
- 发布脚本通过认证 Release 列表查找草稿，再按 release ID 持续核验；即使按标签接口对草稿返回 404，也能完成四项资产的回下载校验后再公开发布。
- GitHub Actions checkout 已升级到使用 Node 24 的固定官方版本，消除旧 Node 20 action 运行时警告。

Windows 安装包继续使用 Tauri 更新签名，暂未配置 Authenticode 发布者签名；macOS 仍未提供经过签名与安装验证的发布包。
