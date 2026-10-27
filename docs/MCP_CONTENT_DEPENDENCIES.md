# Native MCP content dependencies

TodoList parses explicitly requested managed file bytes locally. It does not require Python, Microsoft Office, an OCR application, a cloud service, or a document parser executable. This inventory records the resolved Cargo.lock versions; it does not claim every upstream release or platform has been tested.

| Dependency | License | Primary source | Enabled use |
| --- | --- | --- | --- |
| lopdf 0.46.0 | MIT | [Published manifest/source](https://docs.rs/crate/lopdf/0.46.0/source/Cargo.toml) | Native bounded PDF page/font/stream text extraction; default optional features disabled |
| image 0.25.10 | MIT OR Apache-2.0 | [Published manifest/source](https://docs.rs/crate/image/0.25.10/source/Cargo.toml) | PNG, JPEG, GIF, WebP, BMP only; defaults disabled |
| quick-xml 0.42.0 | MIT | [Published manifest/source](https://docs.rs/crate/quick-xml/0.42.0/source/Cargo.toml) | Strict bounded Office XML reading |
| zip 9.1.2 | MIT | [Published manifest/source](https://docs.rs/crate/zip/9.1.2/source/Cargo.toml) | Office containers, defaults disabled, Rust DEFLATE backend; no system zlib DLL |

Distributions must retain the applicable dependency copyright/license notices. base64 and tempfile are direct runtime dependencies for MCP image-block encoding and short-lived text import staging. The selected parser graph requires Rust 1.88 or newer; Windows compilation and synthetic format regressions are verified. Production installer size and other operating-system packages need their own release acceptance.

Inputs are limited to16MiB. PDF streams/fonts/CMap expansion, Office ZIP entry counts/expanded size, XML size/depth/events and image dimensions/allocation are independently bounded. PDF Form text follows actual calls with inherited resources; recursion16, calls256, cumulative decoded streams32MiB, operations200000 and font encodings256 bound each read, and cycles reject. Used images/inline images or unresolved resources cannot report a complete text search. Archives are never extracted to disk. Macros, scripts, formulas and external relationships are not executed or fetched. File bytes and returned excerpts are untrusted data rather than instructions.

DOCX paragraphs, PPTX slides and XLSX raw/cached values are text extraction, not Office visual-layout rendering. Spreadsheet formulas and number/date formatting are not evaluated. Legacy binary DOC/PPT/XLS remain unsupported. Scanned images and other unreadable document structures require OCR or an explicit partial/error result; text extraction must not claim that unreadable content was fully searched. Image delivery uses standard MCP image blocks and does not prove client rendering or OCR. See [MCP API](MCP_API.md) for current read/search budgets, statuses and platform handle-validation limits.
