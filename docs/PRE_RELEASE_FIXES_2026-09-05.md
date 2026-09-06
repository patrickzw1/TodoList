# 2026-09-05 发布前五项修复

> 以下为修复任务完成时的历史记录。后续发布复核又补上了“前一次操作失败后，排队编辑须重读最新数据库”的回归与修复；最终前端测试增至 35 项。当前发布配置、构建命令和验收范围分别见 `UPDATES.md` 与 `RELEASE_NOTES_v0.1.0.md`。

本轮在 `the repository checkout` 直接开发。项目仍无 HEAD，所有源码仍未跟踪；未执行 commit、push、远程仓库创建、发布、安装包安装或用户级 Codex 配置修改。SQLite 回归仅使用临时数据库；浏览器使用预览数据中的专用测试项目“修复回归 0905”。

## 修复结果

| 问题 | 实现与验证 |
| --- | --- |
| P1 恢复后过期 MCP 写请求被接受 | 新增 SQLite 原子恢复入口，在事务内校验工作区版本并为恢复的任务分配新版本。单行 `task_version_clock` 保留最高版本，覆盖重复恢复、删除后恢复、旧 MCP 仍在运行及并发写入。版本计数只保存一个数，不复制任务历史；schema 2 → 3 保留原任务数据。 |
| P1 完成操作在冲突重试时绕过验收 | 用户操作及重放均检查最新任务；SQLite 的普通保存、MCP 原子修改和幂等修改也检查完成状态变化。旧数据中已有的未确认项不会阻断无关编辑。编辑器未修改的清单不随表单覆盖；修改清单文字时保留最新勾选状态。便签异步保存失败显示明确提示。 |
| P2 新条目继承错误的完成状态 | 去除数组下标兜底，仅未改动的标题复用原 ID 与状态；新增及改名项默认未完成，重复标题按原顺序逐项匹配。子任务和验收标准均覆盖插入、删除、重排、改名及重复标题。纯文本编辑器无法区分同名条目，采用这一确定性规则。 |
| P2 缓存满阻碍已成功的数据库读写 | 桌面 localStorage 仅作为可失败的缓存，缓存异常不阻断 SQLite 结果显示或误报数据库保存失败。桌面跨窗口变更以数据库版本检查为准；仍每 2 秒轻量轮询版本，变化时才读取完整快照。浏览器预览保存失败继续报错、回滚并允许重试。 |
| P2 项目颜色不可见 | 侧栏文件夹图标使用项目颜色，任务分组、任务标签、详情标签与看板项目文字旁显示小色点，保留现有布局。 |

## 实跑验证

- `cargo test --workspace --locked --offline --jobs 1`：33 项通过。包括真实 MCP 服务实现与临时 SQLite 的恢复/过期请求、并发事务、旧版本迁移、删除后恢复及验收限制回归。
- `npm.cmd run test:app`：34 项通过。包括真实 workspace Hook 的内存驱动测试：缓存读写异常、数据库保存成功、版本轮询、完成冲突、编辑器勾选状态冲突及恢复后排队写入。
- `npm.cmd run typecheck`：通过。
- `npm.cmd run test:sites`：4 项通过。
- `cargo fmt --all -- --check`：通过。
- MCP STDIO schema 冒烟指定 `TODOLIST_MCP_EXECUTABLE=target\package-build\release\todolist-mcp.exe`：六个工具通过。脚本现在尊重显式路径，不再覆盖为旧的默认 release 路径。
- Node 测试首次受沙箱 `spawn EPERM` 限制；允许本地测试子进程后通过。Rust 初次失败为 schema 版本断言仍写 2；随 schema 3 更新断言后通过。

## 浏览器验收

本次自建 Vite 服务：`http://127.0.0.1:1421/`，排除 `target` 和 sidecar 目录监视；最终生产资源还通过无文件监视的 Vite preview 补验。验证后关闭了本次两次服务及三个测试标签。

- 先确认首项，再在两组清单开头插入：新项均未完成，原首项仍已完成；删除和重排后状态仍正确。
- 详情、看板状态选择和便签完成入口均提示先确认验收；确认全部验收后，看板可正常完成。
- 所选粉色 `#d84b6b` 的实际渲染为 `rgb(216, 75, 107)`，侧栏、分组、标签及看板可见；1280 像素预览无横向溢出。
- 刷新后完成状态和项目保留；主窗口及便签控制台无 error/warn。
- 最终生产资源：编辑器选择“已完成”但验收未确认时保留表单并显示提示；项目从粉色编辑为 `#258ca6` 后，侧栏、分组及标签立即显示 `rgb(37, 140, 166)`。
- 后补的编辑器并发勾选保留逻辑另有 Hook 回归，不以浏览器普通交互替代并发测试。

## 构建与限制

采用 `CARGO_TARGET_DIR=target\package-build` 隔离现有被 Codex 占用的 debug/release MCP。sidecar 准备脚本已按该目录取源文件，并比对源文件与 `src-tauri/binaries` 副本的 SHA-256 一致。

构建命令：`npm.cmd run build:desktop -- --no-bundle --ci`，配合 `CARGO_NET_OFFLINE=true`、`CARGO_BUILD_JOBS=1`。只生成本地可执行文件，不生成或安装发布安装包。

最终构建成功，桌面产物：`target\package-build\release\todolist-desktop.exe`。

- 修改时间：2026-09-05 21:03:50（Asia/Shanghai）；大小 14,969,344 字节。
- 桌面 EXE SHA-256：`E9B97BA3E04D50D5086312E907369368507229ED36EF474580C63F244027C3CC`。
- 配套 MCP SHA-256：`D7888994034C867FF142566C57BE25593E7AC85F404FBE0765BD8B63F2F6865C`，与复制到 `src-tauri/binaries` 的 sidecar 相同。
- PE 静态检查：桌面 EXE 为 Windows GUI 子系统（2），MCP 为 Console 子系统（3）。
- 最终 `npm run build` 成功；最新前端入口资源为 `index-BgqVS6Kx.js`，已用于最终桌面构建。

未启动新 EXE 访问日常任务库，未实机验证新构建的原生窗口拖动/置顶/最小化、Windows 文件选择器及真实 WebView 与 SQLite 的端到端交互。并发与缓存异常通过上述隔离回归验证。GitHub remote、更新端点和签名公钥仍属于后续发布准备。

保护的 Sites 文件 `.openai/hosting.json`、`worker/index.js`、`scripts/prepare-sites-build.mjs`、`tests/sites-worker.test.mjs` 未编辑；生产构建保留 `dist/client/index.html`、`dist/server/index.js` 和 `dist/.openai/hosting.json`。
