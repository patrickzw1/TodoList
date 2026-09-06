# TodoList Design QA

## Comparison target

- Product visual source: `qa/source-reference.png`
- User feedback source: `qa\feedback-task-editor-before.png` (626 × 839 px)
- Rendered implementation: `qa\implementation-task-editor-redesign.png` (1280 × 720 px)
- Combined comparison: `qa\task-editor-redesign-comparison.png`
- Interactive comparison document: `qa\task-editor-redesign-compare.html`
- Viewport: 1280 × 720 CSS px, devicePixelRatio 1
- Normalization: the implementation modal was cropped from x=320, y=19 to a 640 × 682 component view; the source remains at its native 626 × 839 size. The height difference comes from the shorter implementation viewport, so vertical content density was judged above the fold and through the independently scrolling body rather than by total visible field count.
- State: Today/list view, “运行发布前测试” selected, task editor open, title input focused.

## Full-view comparison evidence

The combined comparison was opened and inspected with both complete modal surfaces in the same visual input. The redesign retains the original editor's field order and two-column information architecture while bringing its frame, header, controls and action footer into the task board's existing light blue-gray design system.

The header and action footer are now fixed visual regions, while the long form scrolls independently. At the 720 px test height the primary and secondary actions remain visible instead of competing with the final text area.

## Focused-region comparison evidence

- Typography: control values no longer inherit the labels' semibold weight. Labels remain compact and legible, while task content uses the same regular optical weight as the main list and detail panel.
- Focus treatment: the former bright double outline is replaced by one blue border and a low-opacity 3 px focus halo. The title focus state remains visible without looking like a browser default.
- Borders and surfaces: controls use a 1 px neutral blue-gray border, an 8 px radius and a subtle off-white surface; hover and focus transitions preserve affordance without increasing visual noise.
- Layout rhythm: the modal uses a 70 px header, 22–26 px content padding, 18 px field rhythm and a 14 px two-column gutter. These values align with the nearby workspace and detail-panel density.
- Scrollbar: the form body's thin rounded blue-gray scrollbar matches the task detail scrollbar and stays visually separated from the fixed footer.
- Actions: the cancel and save buttons share the app's 8 px control radius, compact 13 px text and blue primary token.

## Findings

No actionable P0, P1 or P2 visual differences remain.

- P3: At the minimum supported desktop height, dependencies require a short scroll. The footer stays visible, so this does not block editing or saving.

## Comparison history

1. The user feedback capture showed P2 visual drift: semibold field values, harsh default borders, a bright double focus outline, and a continuous white body without clear header/footer structure.
2. The task-editor-specific styles were revised without changing task data or save behavior. Font weights, borders, focus state, modal regions, field spacing, buttons and scrollbar were aligned to the existing TodoList tokens.
3. The revised editor was captured in the same focused-title state and compared in `task-editor-redesign-comparison.png`. No P0/P1/P2 issue remained in the post-fix visual evidence.

## Interaction verification

- Existing task values populate every field: passed
- Title autofocus and visible focus state: passed
- Form body scroll is independent from header and footer: passed by rendered inspection
- Cancel and save actions remain persistently visible: passed by rendered inspection
- Clicking outside the modal closes it: passed
- Vite runtime output showed no compile or HMR errors during the interaction check

## Required fidelity surfaces

- Fonts and typography: passed; family remains the app system stack, labels use semibold hierarchy, controls use regular weight, and Chinese text no longer appears artificially heavy.
- Spacing and layout rhythm: passed; consistent modal padding, control height, grid gutter, radii and fixed-region boundaries.
- Colors and visual tokens: passed; neutral borders, off-white fields, muted labels and the existing `--blue` token are used consistently.
- Image quality and asset fidelity: not applicable; this form contains no raster or decorative image assets and continues to use the existing Phosphor close icon.
- Copy and content: passed; field names, values, placeholders and actions are unchanged.

## Build verification

- `npm.cmd run typecheck`: passed
- `npm.cmd run test:app`: 18 passed
- `npm.cmd run build`: passed
- `npm.cmd run test:sites`: 4 passed after the production build completed
- Tauri release build with one Cargo job: passed

final result: passed
