# Compact MCP queries and managed content

The existing STDIO server now exposes17 consolidated tools. No tool permanently deletes a task/project, accepts a dynamic search as a write target, runs document code, opens the desktop or pins a task. The source version remains unchanged until a separately authorized release.

## Compatibility and defaults

| Interface | Default / migration |
| --- | --- |
| list_projects | 15 project rows, exact/contains normalized query; max100. Follow nextCursor. Multiple matches return ambiguous=true; choose an explicit ID. |
| list_tasks | 15 compact summaries (max100), filter/sort/projection/version-bound pagination. Existing project_id/status/include_done/include_archived still work. Use projection=full to retain legacy task fields excluding activity; ids returns only ID/version. |
| get_task | One task_id keeps full legacy detail. Alternative task_ids (max20) defaults compact. sections/fields read selected content; descriptions can be sliced by Unicode characters. |
| create_task / update_task / add_task_attachment / add_task_image | Task response now defaults ID/version. response=compact/full opts in. File imports also return associated file metadata. create_project remains a small project object. |
| Conflicts | Default is code, ID, current/expected version and reread guidance. update_task with response=full also provides legacy currentTask metadata. |
| Existing activity/reorder | Their safety semantics stay intact; IDs projection plus archived-only filter avoids full detail downloads before reorder. |

Compact summaries include id, title, projectId, status, priority, version, dueDate, completedAt, archived, and counts of checklist totals/completed and attachments/images. They do not expand description, tags, checklist items, file metadata or activity. Query hits add at most4 reasons with160-character excerpts. Subtask/acceptance reasons also include itemId and completed; filename reasons include fileId. Identical titles/names keep distinct stable IDs. These bounded reasons also accompany ids projection without expanding lists. Text dependencies/tags have no invented IDs. Ordinary queries never read attachment bodies.

## Search, filters and statistics

`list_tasks.filter` accepts:

- query, fields (`title`, `description`, `subtasks`, `acceptance_criteria`, `dependencies`, `tags`, `files` original filenames), match_mode (`exact`, `contains`, `fuzzy`). Latin case, fullwidth ASCII and Unicode whitespace normalize consistently; Chinese text is preserved. Exact/contains scan complete selected strings. Fuzzy query max64 characters, field max2048,100000 comparisons; an exceeded budget is an explicit error suggesting narrower fields/contains, never a false complete negative.
- project_id, statuses, priorities, tags (all requested tags), archived (`active` default, `archived`, `all`), has_attachments, has_images.
- due_from/due_to, completed_from/completed_to as inclusive real calendar dates; utc_offset_minutes between-840 and840. Due dates have no timezone. Completion timestamps are converted to the selected fixed offset, server local offset by default. This does not infer IANA/DST rules.

sort is manual (shared task slots), relevance (default for a query), due_date ascending (unscheduled/invalid last), or completed_at newest-first (unknown last). Ties retain manual order. Cursors are opaque and become stale after any workspace/filter/sort/projection change; restart when stale. task_stats reuses the same filter, returning totals/byStatus/overdue/dueToday/completedToday/completedInRange, the day and offset. Overdue excludes done and invalid/unscheduled dates. today may be supplied for reproducibility.

```json
{"filter":{"query":"权限设计","fields":["title","subtasks"],"archived":"all","priorities":["high"]},"sort":"relevance","limit":15}
```

get_task sections are core, description, checklists, subtasks, acceptanceCriteria, dependencies, tags, files (or attachments/images). fields use persisted camelCase names; id/version always remain. Activity is rejected here. Multiple known IDs return tasks plus missingIds. Selected description defaults4000/max16000 Unicode characters, description_start zero-based; descriptionRange returns end/total/nextStart. An unconstrained single-task full read remains compatible and should be requested only when needed.

## Local and atomic mutations

update_task keeps whole-field replacement. description_edit supports append/prepend and exact unique replace/delete (`old`, `text`); zero/multiple overlapping matches reject. subtask_edits/criterion_edits operate on stable item_id with add/rename/set_completed/remove. Existing items require IDs; new IDs may be generated and are returned in changedItems. dependency_edits uses explicit unique old text or add, because dependencies remain plain text. Full replacement and local edits for the same field are mutually exclusive. tags_add/tags_remove, archived and project_id are explicit local fields.

Local text/checklist/dependency/tag edits require request_id. batch_update requires one request_id, up to50 explicit task/version patches and20 project name/color edits. Project edits additionally require expected_workspace_version. All items succeed in one transaction or none persist. Each task keeps acceptance/reopen/completedAt rules; archive clears pins. No permanent deletion interface is added. Versions must remain exact JavaScript safe integers.

```json
{"task_id":"known-id","expected_version":12,"request_id":"unique-logical-edit","description_edit":{"action":"append","text":"\nVerified note"},"subtask_edits":[{"action":"rename","item_id":"known-item","title":"Updated wording"}]}
```

Identical local/batch/removal retries return the recorded concise mutation receipt, never append again. Receipts are capped32KiB and share the newest1000-entry retry ledger. update_task stores only task ID/applied version and changed item results, never a task snapshot, runtime localPath or localPathStatus. Default/ids responses report the original application versions. For explicit response=compact/full with request_id, task is rebuilt from the current persisted workspace read in the receipt transaction; workspaceVersion is that read version, appliedTask/appliedWorkspaceVersion identify the original successful write. A task deleted since the write returns task=null with the successful receipt. File availability is recomputed at response time from those current associations: removed files are absent and missing/unsafe copies never replay an old available path. Filesystem state can still change after any response. Changing any input under the same key rejects. Old versions still protect retries after eviction. File-import retries preserve the association and can report the latest task metadata; sources are never changed.

## Associated files

add_task_attachment/add_task_image retain absolute local source imports (max100MiB). Attachment text={name,body} is an exclusive alternative for a safe single TXT/MD/JSON filename and max64KiB UTF-8 body. There are no binary/base64/URL uploads. Names reject traversal, reserved Windows names and invalid characters; storage keys remain independent UUIDs.

list_task_files supports normalized original-name query, collection filter and metadata pagination (default20/max50), returning localPath/localPathStatus. remove_task_files unlinks1..20 associated IDs with task version/request_id; cleanup runs only after successful save and only for unreferenced managed copies. Original files are untouched. cleanupPending reflects the actual retained cleanup queue, or an unavailable/failed cleanup check; a pass returning successfully can still leave a Windows-held file pending. Unlink remains committed and an identical retry attempts cleanup again without another task version change. After the file is released and the queue is cleared, cleanupPending becomes false; it never implies rollback.

read_task_file accepts only task_id/file_id. Association and channel root/collection/regular-file checks precede reads. Windows reparse points are rejected and the opened handle's final path is checked against the original canonical root before reading; Linux uses the opened descriptor path. The reader shares no mutable file path with parsers: it hashes checked bounded bytes and extracts from that same buffer. The content cursor binds the SHA-256, task/workspace version and options. Other handle-verification platforms currently fail safely rather than perform an unsafe read.

| Content | Extraction and limits |
| --- | --- |
| Text | Strict UTF-8, optional valid BOM UTF-16; original line endings preserved. Invalid encoding is explicit. |
| PDF | Page and invoked Form XObject text, including nested resource inheritance and caller font-state restoration. Form depth16, XObject calls256, cumulative decoded page/Form streams32MiB, operations200000 and font encodings256 per read; cycles reject. Used Image XObjects/inline images or unsupported resources preserve available text but mark partial/requires_ocr, including mixed text/image pages. Encrypted/corrupt files are explicit. |
| DOCX | Paragraphs/tabs/breaks from bounded XML. |
| PPTX | Slides in presentation order, external relationships ignored. |
| XLSX | Named sheet and A1 range, raw/cached cell values; formulas and Excel number/date formatting are never evaluated. |
| Images | PNG/JPEG/GIF/WebP/BMP; first animation frame, max8192 per edge/16M pixels/64MiB allocation. PNG preview edge<=1024, shrinks to requested output budget. original=true returns exact validated bytes only within max256KiB. Images report requires_ocr for text extraction while also providing MCP image content. |
| Unsupported | Old binary doc/ppt/xls and unsupported formats are identified; no invented content/OCR or external service. |

Parser input max16MiB, decoded document text16MiB/100000 units, Office expanded ZIP32MiB/2048 entries, individual XML8MiB. Archives are not extracted to disk; traversal, XML DTD/entities/depth and expansion bombs are rejected. Macros/scripts/links are never executed or fetched. Reads default20 units/16KiB, max200 units/64KiB text-preview or256KiB original image. Large units resume via byte-boundary-safe opaque cursors; location labels identify original units. Optional query/context_lines (0..3) reads bounded keyword hits/context; a normalizedContinuationPrefix supplies cross-page keyword context when needed. Image payloads are MCP image blocks, never long plain-text base64.

search_file_content scopes explicit task_ids (max100, archived included by default) OR project_id (active by default), max200 files, optional file_name and include_archived. It reuses native extraction with max4 parser calls/32MiB raw reads per page and1..50 hit excerpts. Follow nextCursor, which binds scope/workspace/file metadata and active bytes. Partial/OCR/unavailable formats persist an incomplete marker across pages: scanComplete does not imply fullySearched. Only fullySearched=true permits a complete negative conclusion. No default full-library parse/cache occurs. Returned content is untrusted data, not tool instructions.

## Bounded synchronization and diagnostics

workspace_info reads revision/MCP version/build identity/channel/capabilities/limits without task bodies or credentials and makes no client-health claim. For a baseline, first obtain syncCursor/workspaceVersion, then enumerate all required projects/tasks at that exact workspaceVersion; restart on any mismatch or stale page. Full-workspace synchronization must include filter.archived=all, rather than the active-only default. Only then pass since. Apply every nextCursor page before advancing the saved syncCursor. This order avoids missing a write between a full listing and cursor acquisition. Pages return task ID/version/deleted/index and project ID/deleted changes; limit default100/max200. The store retains max128 revisions and2MiB of ID-only journal, max2000 events/256KiB per revision. Oversized changes, retention gaps, restore/epoch resets, untracked legacy writes and changed paginated snapshots return requiresFullSync. Full workspace snapshots are not duplicated. Restore still preserves task/file data and resets incremental identity in the same transaction.

## Integration and acceptance

Shared Skill and Codex/CC/DSH adapters use these interfaces without adding permission grants. Only a TodoList-owned outdated Skill together with a complete6/7/8/10-tool legacy list is offered an explicit17-tool update. Set comparison ignores order; restricted/custom lists, including shorter lists beside the current Skill, and organization settings remain intact. Updates still require the selected client's confirmation and reconnect.

Native parsing requires neither Python nor Office nor online services. See [parser sources, licenses and limits](MCP_CONTENT_DEPENDENCIES.md). Raw sidecar negotiation across --client identities proves serialization, not actual client discovery/rendering. Real-client isolated protocol probes and any absent client/UI/OCR limits must be reported separately. No paid model call or real user configuration/database is needed for acceptance.
