import type { Workspace } from "./types";

export const seedWorkspace: Workspace = {
  version: 18,
  projects: [
    { id: "personal-site", name: "个人网站发布", color: "#1665e8" },
    { id: "mcp-todo", name: "MCP Todo", color: "#7c5ce7" },
    { id: "research", name: "研究笔记", color: "#269b58" },
  ],
  tasks: [
    {
      id: "publish-docs", projectId: "personal-site", title: "完善发布说明文档", description: "整理本次网站发布范围、回滚方式与维护说明。", status: "todo", priority: "medium", dueLabel: "今天", dueDate: "2026-09-01", tags: ["发布"], source: "手动创建", archived: false, pinned: false, version: 2, subtasks: [], acceptanceCriteria: [{ id: "c1", title: "发布步骤清晰", completed: false }, { id: "c2", title: "回滚路径可执行", completed: false }], attachments: [], images: [], dependencies: [], activity: [{ id: "a1", action: "创建任务", actor: "user", at: "8月31日 10:12" }],
    },
    {
      id: "preflight", projectId: "personal-site", title: "运行发布前测试", description: "完成主流浏览器和部署环境的发布前验证。", status: "in_progress", priority: "high", dueLabel: "今天", dueDate: "2026-09-01", tags: ["测试", "发布"], source: "手动创建", archived: false, pinned: true, version: 4,
      subtasks: [{ id: "s1", title: "检查测试环境配置", completed: true }, { id: "s2", title: "执行冒烟测试", completed: false }, { id: "s3", title: "执行兼容性测试", completed: false }],
      acceptanceCriteria: [{ id: "c3", title: "所有核心功能通过测试", completed: false }, { id: "c4", title: "关键页面在主流浏览器正常显示", completed: false }, { id: "c5", title: "无严重兼容性或崩溃问题", completed: false }], attachments: [], images: [], dependencies: ["部署发布清单"], activity: [{ id: "a2", action: "添加了子任务", actor: "user", at: "8月31日 10:18" }, { id: "a3", action: "优先级更新为高", actor: "user", at: "8月31日 10:40" }],
    },
    {
      id: "screenshots", projectId: "personal-site", title: "更新网站截图", description: "替换发布说明中已过期的界面截图。", status: "todo", priority: "low", dueLabel: "明天", dueDate: "2026-09-02", tags: [], source: "手动创建", archived: false, pinned: false, version: 1, subtasks: [], acceptanceCriteria: [], attachments: [], images: [], dependencies: [], activity: [],
    },
    {
      id: "permissions", projectId: "mcp-todo", title: "完善权限与角色设计", description: "确定本地 MCP 的项目共享范围和读写边界。", status: "todo", priority: "medium", dueLabel: "今天", dueDate: "2026-09-01", tags: ["MCP"], source: "Codex 创建", archived: false, pinned: false, version: 1, subtasks: [], acceptanceCriteria: [{ id: "c6", title: "读写权限可分别控制", completed: false }], attachments: [], images: [], dependencies: [], activity: [{ id: "a4", action: "Codex 创建任务", actor: "codex", at: "9月1日 09:20" }],
    },
    {
      id: "connection-docs", projectId: "mcp-todo", title: "编写连接与权限文档", description: "为 Codex MCP 本地连接整理安装与排错步骤。", status: "todo", priority: "medium", dueLabel: "明天", dueDate: "2026-09-02", tags: ["文档"], source: "Codex 创建", archived: false, pinned: false, version: 1, subtasks: [], acceptanceCriteria: [], attachments: [], images: [], dependencies: [], activity: [],
    },
    {
      id: "conflicts", projectId: "mcp-todo", title: "实现冲突处理规则", description: "用户修改优先；过期版本写入必须重新读取并安全合并。", status: "blocked", priority: "high", dueLabel: "逾期 1 天", dueDate: "2026-08-31", tags: ["同步"], source: "Codex 创建", archived: false, pinned: false, version: 3, subtasks: [], acceptanceCriteria: [{ id: "c7", title: "同字段冲突不静默覆盖", completed: false }], attachments: [], images: [], dependencies: ["事件日志设计"], activity: [],
    },
    {
      id: "llm-paper", projectId: "research", title: "阅读论文：LLM 工具链综述", description: "提炼 MCP、Skill 和本地工具协作的设计模式。", status: "done", priority: "low", dueLabel: "今天", dueDate: "2026-09-01", tags: ["阅读"], source: "手动创建", archived: false, pinned: false, version: 2, subtasks: [], acceptanceCriteria: [], attachments: [], images: [], dependencies: [], activity: [],
    },
    {
      id: "experiment-notes", projectId: "research", title: "整理实验记录", description: "把最近的工具调用实验整理为可检索记录。", status: "todo", priority: "medium", dueLabel: "明天", dueDate: "2026-09-02", tags: [], source: "手动创建", archived: false, pinned: false, version: 1, subtasks: [], acceptanceCriteria: [], attachments: [], images: [], dependencies: [], activity: [],
    },
  ],
};
