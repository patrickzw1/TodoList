# TodoList

一个本地优先、可独立使用，并可选择通过 MCP 与 Codex 互通的 Windows/macOS Todo 桌面应用。

## Windows 下载

在 [GitHub Releases](https://github.com/patrickzw1/TodoList/releases/latest) 下载 `TodoList_0.1.0_x64-setup.exe`。首版面向 Windows x64 日常试用，macOS 尚未提供安装包。

首次启动包含演示项目和任务，可按需归档或清理；任务保存在本机。建议在设置中定期导出 JSON 备份。安装或升级前请退出 TodoList 和使用其 MCP 的 Codex，避免后台 sidecar 占用安装文件。

更新包使用 Tauri 签名校验；本阶段没有 Windows Authenticode 发布者签名，系统可能显示未知发布者提示。已知范围与验证记录见 [v0.1.0 发布说明](docs/RELEASE_NOTES_v0.1.0.md)。

当前首个垂直切片包含：

- 三栏任务台：今日、进行中、多个项目、列表/看板和任务详情
- 新建和编辑任务、创建/编辑项目、当前视图搜索、完成/重新打开、归档与安全删除
- 子任务和验收标准独立确认；有未确认验收标准时不能完成任务
- 看板拖放和状态选择器可直接更新任务状态
- 用户主动置顶的 always-on-top 桌面便签窗口
- 侧栏“桌面便签”可独立打开便签并最小化任务台；行内置顶不最小化，顶部可拖动
- 主窗口关闭后仍可从便签重新打开任务台；最小化时恢复，打开失败时可见提示并重试
- React/TypeScript 浏览器预览和 Tauri 2 桌面壳
- Rust Task Core 与 SQLite WAL 本地存储基础
- 有界的快照存储：只保留当前任务库；旧版冗余全量快照会在首次启动时安全清理并压缩回收空间
- 每个任务只保留最近 100 条活动记录；新记录使用真实时间，旧版无时间记录不再永久显示为“刚刚”
- 带格式版本的 JSON 备份与恢复：系统文件选择、导入预览、明确确认和双层数据校验
- UI 与 MCP 并发写入时自动重读并重放用户操作；空闲时只轮询轻量版本号
- 今日、明日和逾期状态使用本机日期动态计算
- Rust STDIO MCP：读取项目/任务、创建任务和版本安全更新
- 项目内 Codex Skill：冲突时保留用户新修改，不默认重开已完成任务
- 用户确认后的一键 Codex 集成：注册随应用打包的 MCP，并安装用户级 Skill
- 侧边栏显示真实的 Codex 集成配置状态，不把“已配置”误写成实时连接
- 设置页手动检查更新；只安装通过 Tauri 更新签名验证的新版本

Codex 不会自动打开便签，也不会从 Todo 反向启动会话。集成只写当前用户的 `~/.codex/config.toml` 与 `~/.agents/skills/todolist-mcp`，不会修改 Codex 安装目录；连接方式和安全边界见 [Codex integration](docs/CODEX_INTEGRATION.md)。备份格式和文件安全边界见 [Data backup](docs/DATA_BACKUP.md)。

更新采用“下载、验证、重启安装”，不在运行中直接替换文件。正式发布配置和密钥边界见 [Updates](docs/UPDATES.md)。

## 开发命令

```powershell
npm install
npm run dev
npm run typecheck
npm run test:app
npm run test:windows
npm run build
npm run dev:desktop
npm run build:desktop
cargo test --workspace
```

桌面构建需要 Rust stable、Windows WebView2，以及对应平台的签名工具链。

Windows 已验证可生成 MSI 与 NSIS 安装包。macOS 共用同一套 Tauri/React/Rust 源码，但仍需在 macOS 主机上完成签名和安装包验证。
