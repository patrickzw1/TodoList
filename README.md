# TodoList

[English](README.en.md)

TodoList 是本地优先的桌面任务应用。Windows 版可独立使用；用户也可以主动配置 Codex、Claude Code 或 DeepSeek Harness，通过本地 MCP 访问任务。

## Windows 下载与使用

从 [GitHub Releases](https://github.com/patrickzw1/TodoList/releases) 选择稳定版本，下载对应的 `TodoList_<版本>_x64-setup.exe`。目前提供 Windows x64 安装包；macOS 代码共用，但尚无已验证的安装包。安装后可在设置中查看实际版本。

新安装的任务库为空。升级保留已有任务和托管文件；建议定期在设置中导出 JSON 备份。更新包经 Tauri 签名验证，安装仍由用户确认。当前 Windows 安装包没有 Authenticode 发布者签名，系统可能提示“未知发布者”。安装与更新恢复细节见 [更新说明](docs/UPDATES.md)。

主要功能：

- 今日、进行中、全部、项目和已归档视图；列表、看板、视图内搜索与任务排序。
- 任务详情、子任务、验收标准、附件、图片和活动记录；多选、归档及经确认的永久删除。
- 可由用户置顶的独立桌面便签；JSON 备份、恢复和本地 SQLite 存储。
- 桌面界面与 MCP 并发修改时进行版本检查并重放明确的用户操作。

## AI 客户端集成

在应用的“连接与权限”中选择 Codex、Claude Code 或 DeepSeek Harness，再确认配置、更新或移除。三个客户端的集成分别管理；移除一个客户端不会移除其他客户端的集成或任务。集成使用随应用提供的本地 STDIO MCP 和用户级 Skill，不修改客户端安装目录，也不会自动授予工具权限、打开任务台或置顶便签。界面的“已配置”表示磁盘配置已同步，实际连接请在客户端确认。路径和权限边界见 [AI 集成说明](docs/AI_INTEGRATION.md)；Codex 兼容说明见 [Codex 集成说明](docs/CODEX_INTEGRATION.md)。

DeepSeek Harness 桌面版 0.2.0-rc.2 已由用户实际调用 TodoList MCP 的只读 `list_projects`。其余工具及写入操作尚未在该客户端实测。

## 数据位置

Windows 新任务库优先使用用户主目录下的 `%USERPROFILE%\.todolist\app.todolist.desktop\todolist.sqlite`，托管附件位于同目录的 `managed-files`。如果已有安全可用的旧版 `%APPDATA%\app.todolist.desktop\todolist.sqlite`，应用继续使用旧库，不会静默搬迁或清空；路径重定向或数据冲突需要明确迁移。开发版使用独立的 `app.todolist.desktop.dev` 通道，桌面与开发 MCP 共用该通道且不会操作日常任务库。备份与文件规则见 [数据备份说明](docs/DATA_BACKUP.md)。

## 开发与发布

```powershell
npm ci
npm run dev
npm run typecheck
npm run test:app
npm run build
npm run dev:desktop
cargo test --workspace
```

浏览器预览默认空白；显式使用 `?demo=1` 才加载演示数据。开发 MCP 名为 `todolist_dev`；连接前运行 `npm run build:sidecar`。普通本地桌面构建也使用开发通道，只有显式合并 `src-tauri/tauri.release.conf.json` 的正式构建使用生产通道。Windows 桌面开发还需要 Rust stable 和 WebView2。

推送到 `main` 或创建拉取请求只运行验证。正式 Windows 包由 GitHub Actions 的 Release 工作流生成：需要一个已存在、位于 `main`、与源码版本相符的稳定 `vMAJOR.MINOR.PATCH` 标签；推送该标签会触发发布，也可在工作流中手动选择这个已有标签。签名与发布细节见 [更新与发布说明](docs/UPDATES.md)。
