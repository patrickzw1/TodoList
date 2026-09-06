---
name: todolist-mcp
description: Read, create, and safely update tasks in the local TodoList app through its MCP server. Use when the user asks to save a plan to TodoList, inspect TodoList progress, or synchronize task changes with TodoList.
---

# TodoList MCP

Use the `todolist` MCP server as an optional bridge to the independent local TodoList app.

The `todolist` server is for daily tasks in the installed app. The TodoList source repository also provides `todolist_dev`, which uses an independent development database. Use `todolist_dev` only when the user explicitly asks for development testing; never silently fall back between the two servers. If the requested server is unavailable, report that its integration needs configuring.

## Workflow

1. Call `list_projects` when the target project id is unknown. If multiple projects are plausible, ask the user which one to use. Use `create_project` only when the user asked for a new project or clearly approved creating one.
2. Call `list_tasks` or `get_task` before changing an existing task. Follow `nextCursor` until it is null when the full filtered result matters.
3. Use `create_task` for new work. Keep titles concise and put detail in description, subtasks, dependencies, and acceptance criteria.
4. For `create_project` and `create_task`, generate one stable, unique `request_id` for the logical operation and reuse that exact value only when retrying the same input. If the input changes, use a new value.
5. Use `update_task` with the latest returned task `version` as `expected_version`.
6. Read the changed task again when the result matters to the conversation.

Acceptance criteria may be supplied as short strings when creating a task. When replacing criteria on an existing task, reuse the latest criterion `id`, `title`, and `completed` values for unchanged items so user confirmations are preserved. `list_tasks` excludes archived tasks unless `include_archived` is true, returns at most 50 tasks by default, and accepts a maximum `limit` of 100.

## Attachments and images

Task details have separate `attachments` and `images` collections. `get_task` and `list_tasks` return metadata: `id`, `originalName`, `mediaType`, `size`, `storageKey`, and `addedAt`. These results do not include file content, and `storageKey` is an internal reference, not a source path to open or upload.

The user adds, removes, opens, and previews files in the desktop task-detail panel. The current MCP tools cannot upload, import, remove, or preview files. If asked to attach a file, explain this limit and direct the user to the desktop attachment or image section. Do not claim that writing a path or data URI in a task description attaches a file. Do not edit SQLite or the managed-file directory to bypass this limit.

`update_task` automatically preserves existing attachments and images while updating supported task fields. Do not attempt to replace these collections through other fields.

## Conflict rules

- On `version_conflict`, read the task again.
- Preserve newer user edits. Merge fields that do not conflict.
- If both the user and Codex changed the same field, keep the user's value and skip that field unless the user explicitly asked to replace it.
- Never reopen a completed task unless the user explicitly asked. Only then set `allow_reopen_completed` to true.
- Before setting a task to `done`, verify that every acceptance criterion is completed. If any remain incomplete, leave the task open and report what still needs confirmation.
- Do not change or request pin state. MCP-created tasks must stay unpinned and must not open the desktop note.

This integration does not trigger, resume, or modify Codex conversations from TodoList events.
