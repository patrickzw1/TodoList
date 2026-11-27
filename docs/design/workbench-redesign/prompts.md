# TodoList 重设计出图记录

2026-11-27。画布 1536 × 1024，数据与选中任务见[设计简报](brief.md)。[当前候选与旧稿](index.html)保留可查看的原图。

## A v0.1

从简报出图。

```text
Create ONE original high-fidelity desktop application UI design image, a crisp native software screenshot, exactly 1536×1024 px landscape 3:2, edge-to-edge app view, straight-on 1:1. No external background, person, device frame, laptop, OS controls, branding besides the exact name 任务台, annotation, watermark, shadows outside the app, gradients, or labels for the design direction. Typography is an extremely sharp simplified-Chinese system sans, art-directed like carefully typeset product design, consistent sizes and pixel-aligned controls. Render the EXACT simplified-Chinese copy below. Legibility and likeness to a usable app matter more than decoration. Original TodoList look, no cloning another app.

State: project task LIST for the selected project 官网体验改版, with selected task row 2 and the task details already open. Fictional data, local Friday 2026-11-27. Same seven records in this order, all visible in the project list. Never invent, repeat or translate a task. Render text and date exactly:
1 整理首屏文案与版式 | 11月27日 | hollow circle
2 核对新版中文首页发布清单并补齐移动端和桌面端验收证据 | 11月27日 | blue in-progress circle; selected and pinned
3 验证下载页链接 | 11月27日 | amber blocked circle
4 更新常见问题和截图 | 11月30日 | hollow circle
5 整理移动端反馈 | 12月1日 | hollow circle
6 归集上次走查记录 | 11月26日完成 | green checked circle
7 确认导航和按钮文字 | 11月25日完成 | green checked circle

Required working elements, small but legible:
Left: 任务台, blue 新建任务, 今日, 全部, 进行中, 已归档. Section 项目 with 官网体验改版 (active), 内容运营, 设计素材. Footer 桌面便签, 连接与权限, 设置 and 本地已保存. Project context also above the task list. Toolbar 搜索, 排序 with 手动顺序, 列表 and 看板, with 列表 active. Rows have petite gray drag grip, independent square batch checkbox (unchecked), circular completion/status icon, title, compact pin, date. Dates for completed rows use completed dates as written, while other dates are due dates. Selected row isn't completed or batch-selected. Footer count 共 7 项.
Right: one fixed opaque floating detail pane from the right, with a soft left shadow over the list canvas, without dark backdrop. It must feel like a panel overlay; no title/control clipping, no resizing implied in the root workspace. Header breadcrumb 官网体验改版, 4 distinct simple icons expand, blue pinned pushpin immediately before pencil edit, X close. The full task title in detail is exactly 核对新版中文首页发布清单并补齐移动端和桌面端验收证据. Must wrap in detail, no ellipsis there. Details show status 进行中, 高优先级, 截止 11月27日, and these 5 distinct tags: 发布, 跨端验收, 中文长文案, 视觉回归, 兼容性与无障碍. Tags wrap neatly if needed. Section 描述 with this exact sentence: 逐项核对桌面与手机端截图，补充流程和异常状态证据。 Next section 附件 1, row 发布核对表.pdf and 92 KB, small attach/open icon and an add action. Then 子任务 1/2, checked 检查宽屏首屏 and unchecked 录入窄屏截图. Then 验收标准 with unchecked 截图与实际版本一致. Bottom footer of the pane is fixed visually separate, with 归档 and the only pane primary button 标记完成. Don't draw a Start, Send to AI, a calendar action, invented workflow, avatar, chart, or other UI copy. No fake English or placeholder text. Functional structures are anchors, compose the visual design from a blank canvas.

Design direction: 快扫工作台. The project list is the first read and stays visually dominant; use a crisp compact editorial index across most of the canvas. Distinct composition: a single contiguous full-height cool slate-tinted sidebar roughly 208px wide with clear nav sections; to its right a wide, uninterrupted shallow project header and a horizontal toolbar including search, sort, and list/board. Use a simple thin-lined table surface, seven rows 54–58px high, consistent horizontal date alignment, very little vertical overhead, subtle column header labels 任务 and 日期, tabular digits, multiple visible controls aligned with a precise grid. Row 2 uses a cool pale blue rectangular highlight with a thin blue inset edge and blue pin; no floating cards. Selected title can wrap to two lines in list if needed. The details are a comparatively slim inspector 500–520px wide flush right on top of the list canvas, with compact grouped date/status and tags around full two-line title, efficient vertical spacing, then 24px line gaps, sections separated by fine rules. Native low-radius buttons and a decisive cobalt (#235AD6) used only where actionable, ink #1B2B42, mainstream modern light Windows desktop, polished subtle blue tint, quiet gray dividers. Give the list labels 14–15px-like scale and the precise visual structure of an information-dense professional tool without miniature unreadable UI. The visual area is broad, rectangular and disciplined, no banner. Keep dates inside the exposed list, independent to title. Create a finished app screenshot, no page framing or design agency presentation.
```

## A v0.2

以 A v0.1 为输入，修正只读元数据控件。

```text
Perform a very local edit on this exact 1536×1024 TodoList draft. Keep the whole app pixel-close and preserve every title, date, icon and row outside two small areas. Area one near top left of the RIGHT inspector: remove the tiny left-facing chevron just before the breadcrumb 官网体验改版, paint its background as the surrounding plain header. Keep breadcrumb words and other four icons unchanged. Area two between the full task title and the 5 tags, only the short status metadata row in the RIGHT pane: remove the downward chevrons and outlined control boxes around status and priority/date. Show the SAME 3 entries as plain read-only inline metadata, with a blue progress circle followed by 进行中, separator, red small up-priority mark followed by 高优先级, separator, 截止 11月27日. No other changes whatsoever. Never touch the left pane, list, dates, attachments, checklist, footer, or five tags. In particular keep the title 核对新版中文首页发布清单并补齐移动端和桌面端验收证据 exact and the seven rows complete. High-fidelity native software screenshot, no margins, same 3:2 canvas 1536×1024.
```

## A v0.3

以 A v0.2 为输入，加入长描述默认态。

```text
Edit only the RIGHT inspector body of the supplied 1536×1024 landscape Chinese TodoList UI mock. Preserve its entire visual style, rendering sharpness, all seven list titles, dates, navigation, selected pinned row, header, full title and metadata, the five tags and fixed footer. Keep the image dimensions exactly 1536×1024. This is still the same selected task 核对新版中文首页发布清单并补齐移动端和桌面端验收证据, and all task data outside description remains byte-for-byte in wording.

In the right panel's 描述 section, replace the former short paragraph with the COLLAPSED preview of this long multi-paragraph text. Render EXACTLY FOUR visible wrapped lines of description body in this mock, preserving the first four explicit line starts:
逐项核对桌面与手机端截图。
补充异常状态和复现步骤。
证据保存路径：
D:\QA\WebsiteLaunch\2026-11-27\DesktopAndMobile…
The ellipsis is a display crop of a continuing Windows path. Display a small plain blue text button 展开全文 immediately below the fourth line; no icon, large container, or duplicate link. Never show the later paragraphs in this collapsed view. Keep the whole collapsed section neat and readable. The full SOURCE description for context, which must not all appear, is:
逐项核对桌面与手机端截图。
补充异常状态和复现步骤。
证据保存路径：
D:\QA\WebsiteLaunch\2026-11-27\DesktopAndMobile\FinalEvidence\HomepageReview

核对发布清单，记录需补拍的场景。
确认交付记录后再归档。

Place 附件 1, 发布核对表.pdf and 92 KB, and both checklist sections beneath the collapsed preview. Fit them above the fixed footer using the vertical space already in the pane, slightly compacting section spacing only as needed. Both attachment and the start of 子任务 1/2 must read at a glance. Keep checked 检查宽屏首屏 and unchecked 录入窄屏截图, then 验收标准 with unchecked 截图与实际版本一致. Preserve read-only 进行中, 高优先级, 截止 11月27日 with no chevrons. Do not change left-side content, row ordering or dates. Confirm pixel-sharp simplified Chinese, exact text and no placeholder.
Maintain this fast scan original's blue-gray single integrated sidebar, tightly lined date columns, pale blue rectangular selected row, and slim efficiency-first detail composition.
```

## B v0.1

从简报独立出图。

```text
Create ONE original high-fidelity desktop application UI design image, a crisp native software screenshot, exactly 1536×1024 px landscape 3:2, edge-to-edge app view, straight-on 1:1. No external background, person, device frame, laptop, OS controls, branding besides the exact name 任务台, annotation, watermark, shadows outside the app, gradients, or labels for the design direction. Typography is an extremely sharp simplified-Chinese system sans, art-directed like carefully typeset product design, consistent sizes and pixel-aligned controls. Render the EXACT simplified-Chinese copy below. Legibility and likeness to a usable app matter more than decoration. Original TodoList look, no cloning another app.

State: project task LIST for the selected project 官网体验改版, with selected task row 2 and the task details already open. Fictional data, local Friday 2026-11-27. Same seven records in this order, all visible in the project list. Never invent, repeat or translate a task. Render text and date exactly:
1 整理首屏文案与版式 | 11月27日 | hollow circle
2 核对新版中文首页发布清单并补齐移动端和桌面端验收证据 | 11月27日 | blue in-progress circle; selected and pinned
3 验证下载页链接 | 11月27日 | amber blocked circle
4 更新常见问题和截图 | 11月30日 | hollow circle
5 整理移动端反馈 | 12月1日 | hollow circle
6 归集上次走查记录 | 11月26日完成 | green checked circle
7 确认导航和按钮文字 | 11月25日完成 | green checked circle

Required working elements, small but legible:
Left: 任务台, blue 新建任务, 今日, 全部, 进行中, 已归档. Section 项目 with 官网体验改版 (active), 内容运营, 设计素材. Footer 桌面便签, 连接与权限, 设置 and 本地已保存. Project context also above the task list. Toolbar 搜索, 排序 with 手动顺序, 列表 and 看板, with 列表 active. Rows have petite gray drag grip, independent square batch checkbox (unchecked), circular completion/status icon, title, compact pin, date. Dates for completed rows use completed dates as written, while other dates are due dates. Selected row isn't completed or batch-selected. Footer count 共 7 项.
Right: one fixed opaque floating detail pane from the right, with a soft left shadow over the list canvas, without dark backdrop. It must feel like a panel overlay; no title/control clipping, no resizing implied in the root workspace. Header breadcrumb 官网体验改版, 4 distinct simple icons expand, blue pinned pushpin immediately before pencil edit, X close. The full task title in detail is exactly 核对新版中文首页发布清单并补齐移动端和桌面端验收证据. Must wrap in detail, no ellipsis there. Details show status 进行中, 高优先级, 截止 11月27日, and these 5 distinct tags: 发布, 跨端验收, 中文长文案, 视觉回归, 兼容性与无障碍. Tags wrap neatly if needed. Section 描述 with this exact sentence: 逐项核对桌面与手机端截图，补充流程和异常状态证据。 Next section 附件 1, row 发布核对表.pdf and 92 KB, small attach/open icon and an add action. Then 子任务 1/2, checked 检查宽屏首屏 and unchecked 录入窄屏截图. Then 验收标准 with unchecked 截图与实际版本一致. Bottom footer of the pane is fixed visually separate, with 归档 and the only pane primary button 标记完成. Don't draw a Start, Send to AI, a calendar action, invented workflow, avatar, chart, or other UI copy. No fake English or placeholder text. Functional structures are anchors, compose the visual design from a blank canvas.

Design direction: 今日专注. The date and selected task become the strongest forms through type, information order and space. Distinct composition: left navigation is split into an approximately 84px slim icon-and-short-label action rail and an always visible ~150px quiet vertical project index. Use easy-to-read icons above their labels in the rail, create at top, utility at bottom. Light warm ivory paper background, almost white reading panel, slight neutral rule, desaturated ink and deep blue accent #2758B3. In the project lane, put the active project name first, then an editorial date headline 11月27日 周五 with quiet small 今日到期 3 as noninteractive context, and a compact project list toolbar immediately above seven loose, generous rows. List items have a readable main line with dates as secondary text immediately beside or beneath the title instead of a spreadsheet column. Selected task is an isolated pale blue row with an inset outline and small blue pinned mark, showing a quiet selected cue among plain borderless rows. Alternate rows remain white. The focal region is a larger 590–630px opaque right reading pane with 36px side padding, anchored to the viewport and subtle left shadow. Give the selected task's exact full text two easy-to-read Chinese lines and bold ~30px typesetting. Set the description directly under the status/date to make a reading rhythm, followed by neat tag wrapping, attachment, checklists. Sections should feel like a work note with line length and open breathing room, never cards in cards. Footer fixed and distinct. Readable useful copy and ample whitespace instead of flourishes. Use original editorial desktop styling, do not add workflow buttons. Show all seven project tasks in the same ordered dataset as in the brief.
In the task detail, status, priority and deadline are plain read-only metadata, not buttons. Do not render disclosure chevrons near metadata or a back arrow on the breadcrumb. Editing is only via the upper-right pencil.
```

## B v0.2

以 B v0.1 为输入，加入长描述默认态。

```text
Edit only the RIGHT inspector body of the supplied 1536×1024 landscape Chinese TodoList UI mock. Preserve its entire visual style, rendering sharpness, all seven list titles, dates, navigation, selected pinned row, header, full title and metadata, the five tags and fixed footer. Keep the image dimensions exactly 1536×1024. This is still the same selected task 核对新版中文首页发布清单并补齐移动端和桌面端验收证据, and all task data outside description remains byte-for-byte in wording.

In the right panel's 描述 section, replace the former short paragraph with the COLLAPSED preview of this long multi-paragraph text. Render EXACTLY FOUR visible wrapped lines of description body in this mock, preserving the first four explicit line starts:
逐项核对桌面与手机端截图。
补充异常状态和复现步骤。
证据保存路径：
D:\QA\WebsiteLaunch\2026-11-27\DesktopAndMobile…
The ellipsis is a display crop of a continuing Windows path. Display a small plain blue text button 展开全文 immediately below the fourth line; no icon, large container, or duplicate link. Never show the later paragraphs in this collapsed view. Keep the whole collapsed section neat and readable. The full SOURCE description for context, which must not all appear, is:
逐项核对桌面与手机端截图。
补充异常状态和复现步骤。
证据保存路径：
D:\QA\WebsiteLaunch\2026-11-27\DesktopAndMobile\FinalEvidence\HomepageReview

核对发布清单，记录需补拍的场景。
确认交付记录后再归档。

Place 附件 1, 发布核对表.pdf and 92 KB, and both checklist sections beneath the collapsed preview. Fit them above the fixed footer using the vertical space already in the pane, slightly compacting section spacing only as needed. Both attachment and the start of 子任务 1/2 must read at a glance. Keep checked 检查宽屏首屏 and unchecked 录入窄屏截图, then 验收标准 with unchecked 截图与实际版本一致. Preserve read-only 进行中, 高优先级, 截止 11月27日 with no chevrons. Do not change left-side content, row ordering or dates. Confirm pixel-sharp simplified Chinese, exact text and no placeholder.
Maintain this calm planning original's slim action rail and separate project list, warm paper background, large project-date heading, generous task rows with dates below, pale blue rounded selected row, and broad reading pane. Keep the bottom-most unchecked acceptance item visible above the footer by utilizing open space, without shrinking Chinese to tiny type.
```

## 局部对照 v0.1

按两态场景独立出图。

```text
Use the supplied TodoList mock as a style reference, not as a canvas to keep. Make ONE separate 1536×1024 px landscape design comparison showing only description/attachment/subtask area of the same inspector at the SAME width in two side-by-side panels. A quiet professional design-review sheet, warm white like the mock, large readable Chinese, align headings and use precise thin rules. No navigation, no decorations, no external brand or date badges. Two clear captions outside the crops, left 默认收起, right 展开后. The two panels start with 描述 aligned on the same baseline.

Left panel shows EXACTLY four actual lines of description:
逐项核对桌面与手机端截图。
补充异常状态和复现步骤。
证据保存路径：
D:\QA\WebsiteLaunch\2026-11-27\DesktopAndMobile…
Followed by the small blue text control 展开全文. Then narrow section 附件 1 with file 发布核对表.pdf and 92 KB on standard file line. Then 子任务 1/2 with one checked 检查宽屏首屏 and one unchecked 录入窄屏截图.

Right panel shows the exact full description with the same lines and a well-wrapped path split across two physical lines, NO ellipsis and NO missing characters. Preserve the blank line after the path. Body to render exactly:
逐项核对桌面与手机端截图。
补充异常状态和复现步骤。
证据保存路径：
D:\QA\WebsiteLaunch\2026-11-27\DesktopAndMobile\FinalEvidence\HomepageReview

核对发布清单，记录需补拍的场景。
确认交付记录后再归档。
Follow with a blue text control 收起 below the final line. Then show the identical attachment and 子任务 1/2 below it at the naturally lower vertical positions. Do not imply a modal or separate details navigation. The two panes should be identical in width, font and icon style so the useful difference is the expanded content and displacement of the attachment.

Below these two panels add one small plain crop caption 短描述 and show one line 核对首页文案。 followed immediately by the next section heading 附件, with no blue expand/collapse link anywhere in the short sample. This is an auxiliary sample on the same comparison sheet, not a third mock. Accurate simplified Chinese and exact fictional Windows path. Edge-to-edge clean white presentation.
```

## 局部对照 v0.2

以局部对照 v0.1 为输入，修正收起态与附件之间的空白。

```text
Edit this exact 1536×1024 comparison sheet. Do not regenerate its text, structure, or visual style. Only move the entire attachment-and-subtasks group UP about 100 pixels inside the LEFT panel, bringing the 附件 1 header to about y=350, about 22 pixels below the existing blue 展开全文 text. Move the file row, 子任务 1/2 heading and two checkbox rows with it as a group. Remove the blank spacer where the group used to be and keep the panel margins. The point is to show that collapsing the description frees height and the following section moves up naturally, with no reserved blank. The entire RIGHT expanded pane and bottom 短描述 sample stay at their original coordinates unchanged. Preserve the exact all Chinese and path, 4 collapsed lines, no ellipsis in expanded, 1:1 canvas; no redrawing unrelated areas.
```
