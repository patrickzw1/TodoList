import { isTauri, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  ArrowLeft, ArrowRight, ArrowSquareOut, File, Image as ImageIcon, MagnifyingGlassMinus,
  MagnifyingGlassPlus, Plus, Trash, WarningCircle, X,
} from "@phosphor-icons/react";
import { ChangeEvent, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { ManagedFile } from "./types";

type FileCollection = "attachments" | "images";

const ATTACHMENT_PREVIEW_COUNT = 3;
const imageFilters = [{ name: "图片", extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"] }];

function readableBytes(size: number) {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${Math.max(1, Math.round(size / 1024))} KB`;
  return `${(size / 1024 / 1024).toFixed(size >= 10 * 1024 * 1024 ? 1 : 2)} MB`;
}

function fileNameParts(name: string) {
  const dot = name.lastIndexOf(".");
  return dot > 0 && dot < name.length - 1 ? { stem: name.slice(0, dot), extension: name.slice(dot) } : { stem: name, extension: "" };
}

function typeLabel(file: ManagedFile) {
  const extension = fileNameParts(file.originalName).extension.slice(1).toUpperCase();
  return extension || file.mediaType || "文件";
}

function browserFile(file: globalThis.File, data: string): ManagedFile {
  return {
    id: crypto.randomUUID(),
    originalName: file.name,
    mediaType: file.type || "application/octet-stream",
    size: file.size,
    storageKey: data,
    addedAt: new Date().toISOString(),
  };
}

function readBrowserFile(file: globalThis.File) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error("读取文件失败"));
    reader.onload = () => resolve(String(reader.result));
    reader.readAsDataURL(file);
  });
}

function useManagedImage(file: ManagedFile | null) {
  const [state, setState] = useState<{ url: string; error: string }>({ url: "", error: "" });
  useEffect(() => {
    let cancelled = false;
    setState({ url: "", error: "" });
    if (!file) return () => { cancelled = true; };
    if (!isTauri()) {
      setState({ url: file.storageKey, error: "" });
      return () => { cancelled = true; };
    }
    void invoke<string>("read_managed_image", { storageKey: file.storageKey })
      .then((url) => { if (!cancelled) setState({ url, error: "" }); })
      .catch((reason) => { if (!cancelled) setState({ url: "", error: reason instanceof Error ? reason.message : String(reason) }); });
    return () => { cancelled = true; };
  }, [file?.id, file?.storageKey]);
  return state;
}

function ImageCard({ file, layer, current, onOpen }: { file: ManagedFile; layer: number; current: boolean; onOpen?: () => void }) {
  const image = useManagedImage(file);
  const content = image.url
    ? <img src={image.url} alt={current ? file.originalName : ""} />
    : <span className={image.error ? "image-unavailable" : "image-loading"}>{image.error ? <><WarningCircle />图片不可用</> : "正在载入图片…"}</span>;
  return current
    ? <button type="button" className="image-stack-card is-current" style={{ "--stack-layer": layer } as React.CSSProperties} onClick={onOpen} title="打开大图">{content}<span className="image-open-hint"><MagnifyingGlassPlus />打开大图</span></button>
    : <div className="image-stack-card" aria-hidden="true" style={{ "--stack-layer": layer } as React.CSSProperties}>{content}</div>;
}

function ImageViewer({ files, index, onIndex, onClose }: { files: ManagedFile[]; index: number; onIndex: (index: number) => void; onClose: () => void }) {
  const file = files[index] ?? null;
  const image = useManagedImage(file);
  const [mode, setMode] = useState<"fit" | "actual">("fit");
  const [zoom, setZoom] = useState(100);
  const [naturalWidth, setNaturalWidth] = useState(0);
  const closeButton = useRef<HTMLButtonElement>(null);
  const viewer = useRef<HTMLDivElement>(null);
  useEffect(() => {
    closeButton.current?.focus();
    const key = (event: KeyboardEvent) => {
      if (event.key === "Tab") {
        const buttons = [...viewer.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? []];
        const first = buttons[0];
        const last = buttons.at(-1);
        if (event.shiftKey && (document.activeElement === first || !viewer.current?.contains(document.activeElement))) {
          event.preventDefault(); last?.focus();
        } else if (!event.shiftKey && (document.activeElement === last || !viewer.current?.contains(document.activeElement))) {
          event.preventDefault(); first?.focus();
        }
      }
      if (event.key === "Escape") onClose();
      if (event.key === "ArrowLeft" && index > 0) onIndex(index - 1);
      if (event.key === "ArrowRight" && index < files.length - 1) onIndex(index + 1);
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [files.length, index, onClose, onIndex]);
  useEffect(() => { setMode("fit"); setZoom(100); setNaturalWidth(0); }, [file?.id]);

  return createPortal(
    <div ref={viewer} className="image-viewer" role="dialog" aria-modal="true" aria-label={`查看图片 ${file?.originalName ?? ""}`} onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <header>
        <div><strong title={file?.originalName}>{file?.originalName}</strong><span>{index + 1}/{files.length}</span></div>
        <div className="image-viewer-tools">
          <button type="button" onClick={() => { setMode("fit"); setZoom(100); }} aria-pressed={mode === "fit"}>适应窗口</button>
          <button type="button" onClick={() => { setMode("actual"); setZoom(100); }} aria-pressed={mode === "actual"}>原始尺寸</button>
          <button type="button" aria-label="缩小" onClick={() => { setMode("actual"); setZoom((value) => Math.max(25, value - 25)); }}><MagnifyingGlassMinus /></button>
          <span>{zoom}%</span>
          <button type="button" aria-label="放大" onClick={() => { setMode("actual"); setZoom((value) => Math.min(300, value + 25)); }}><MagnifyingGlassPlus /></button>
          <button ref={closeButton} type="button" aria-label="关闭大图" onClick={onClose}><X /></button>
        </div>
      </header>
      <div className="image-viewer-canvas">
        {image.url ? <img className={mode === "fit" ? "is-fit" : "is-actual"} style={mode === "actual" && naturalWidth ? { width: `${Math.round(naturalWidth * zoom / 100)}px` } : undefined} src={image.url} alt={file?.originalName ?? ""} onLoad={(event) => setNaturalWidth(event.currentTarget.naturalWidth)} /> : <div className="image-viewer-error"><WarningCircle />{image.error || "正在载入图片…"}</div>}
      </div>
      {files.length > 1 && <>
        <button type="button" className="viewer-step previous" aria-label="上一张图片" disabled={index === 0} onClick={() => onIndex(index - 1)}><ArrowLeft /></button>
        <button type="button" className="viewer-step next" aria-label="下一张图片" disabled={index === files.length - 1} onClick={() => onIndex(index + 1)}><ArrowRight /></button>
      </>}
    </div>,
    document.body,
  );
}

export function TaskFileSections({ taskId, attachments, images, onAdd, onRemove }: {
  taskId: string;
  attachments: ManagedFile[];
  images: ManagedFile[];
  onAdd: (kind: FileCollection, file: ManagedFile) => void;
  onRemove: (kind: FileCollection, fileId: string) => void;
}) {
  const [attachmentsExpanded, setAttachmentsExpanded] = useState(false);
  const [imageIndex, setImageIndex] = useState(0);
  const [viewerOpen, setViewerOpen] = useState(false);
  const viewerReturnFocus = useRef<HTMLElement | null>(null);
  const [importing, setImporting] = useState<FileCollection | null>(null);
  const [message, setMessage] = useState<{ kind: FileCollection; text: string } | null>(null);
  const [unavailable, setUnavailable] = useState<Set<string>>(() => new Set());
  const attachmentInput = useRef<HTMLInputElement>(null);
  const imageInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setAttachmentsExpanded(false);
    setImageIndex(0);
    setViewerOpen(false);
    setMessage(null);
    setUnavailable(new Set());
  }, [taskId]);
  useEffect(() => setImageIndex((current) => Math.min(current, Math.max(0, images.length - 1))), [images.length]);

  const importPaths = async (kind: FileCollection) => {
    setMessage(null);
    if (!isTauri()) {
      (kind === "images" ? imageInput : attachmentInput).current?.click();
      return;
    }
    const selected = await open({ multiple: true, directory: false, ...(kind === "images" ? { filters: imageFilters } : {}) });
    const paths = typeof selected === "string" ? [selected] : selected ?? [];
    if (!paths.length) return;
    setImporting(kind);
    try {
      for (const sourcePath of paths) {
        const file = await invoke<ManagedFile>("import_managed_file", { taskId, sourcePath, kind: kind === "images" ? "image" : "attachment" });
        onAdd(kind, file);
      }
    } catch (reason) {
      setMessage({ kind, text: `导入失败：${reason instanceof Error ? reason.message : String(reason)}` });
    } finally {
      setImporting(null);
    }
  };

  const importBrowser = async (kind: FileCollection, event: ChangeEvent<HTMLInputElement>) => {
    const files = [...(event.target.files ?? [])];
    event.target.value = "";
    if (!files.length) return;
    setImporting(kind);
    setMessage(null);
    try {
      for (const file of files) {
        if (kind === "images" && !file.type.startsWith("image/")) throw new Error(`${file.name} 不是图片`);
        onAdd(kind, browserFile(file, await readBrowserFile(file)));
      }
    } catch (reason) {
      setMessage({ kind, text: `导入失败：${reason instanceof Error ? reason.message : String(reason)}` });
    } finally {
      setImporting(null);
    }
  };

  const openAttachment = async (file: ManagedFile) => {
    try {
      if (isTauri()) await invoke("open_managed_file", { storageKey: file.storageKey });
      else window.open(file.storageKey, "_blank", "noopener,noreferrer");
      setUnavailable((current) => { const next = new Set(current); next.delete(file.id); return next; });
    } catch (reason) {
      setUnavailable((current) => new Set(current).add(file.id));
      setMessage({ kind: "attachments", text: `打开失败：${reason instanceof Error ? reason.message : String(reason)}` });
    }
  };

  const visibleAttachments = attachmentsExpanded ? attachments : attachments.slice(0, ATTACHMENT_PREVIEW_COUNT);
  const stack = useMemo(() => images.slice(imageIndex, imageIndex + 3), [imageIndex, images]);
  const openViewer = () => { viewerReturnFocus.current = document.activeElement as HTMLElement | null; setViewerOpen(true); };
  const closeViewer = () => {
    setViewerOpen(false);
    window.requestAnimationFrame(() => viewerReturnFocus.current?.focus());
  };

  return <>
    <section className="detail-section file-section">
      <header><h3>附件 <span>{attachments.length}</span></h3><button type="button" onClick={() => void importPaths("attachments")} disabled={importing !== null}><Plus />{importing === "attachments" ? "导入中…" : "添加附件"}</button></header>
      <input ref={attachmentInput} className="managed-file-input" type="file" multiple onChange={(event) => void importBrowser("attachments", event)} />
      {visibleAttachments.length ? <div className="attachment-list">{visibleAttachments.map((file) => {
        const name = fileNameParts(file.originalName);
        return <div className={`attachment-row ${unavailable.has(file.id) ? "is-unavailable" : ""}`} key={file.id}>
          <File weight="fill" />
          <button type="button" className="attachment-name" title={file.originalName} onClick={() => void openAttachment(file)}><span>{name.stem}</span><b>{name.extension}</b><small>{unavailable.has(file.id) ? "文件不可用" : `${typeLabel(file)} · ${readableBytes(file.size)}`}</small></button>
          <button type="button" className="attachment-action" aria-label={`打开 ${file.originalName}`} title="使用默认应用打开" onClick={() => void openAttachment(file)}><ArrowSquareOut /></button>
          <button type="button" className="attachment-action remove" aria-label={`移除 ${file.originalName}`} title="移除托管副本" onClick={() => onRemove("attachments", file.id)}><Trash /></button>
        </div>;
      })}</div> : <button type="button" className="empty-file-add" onClick={() => void importPaths("attachments")}><Plus />添加第一个附件</button>}
      {attachments.length > ATTACHMENT_PREVIEW_COUNT && <button type="button" className="show-all-files" onClick={() => setAttachmentsExpanded((value) => !value)}>{attachmentsExpanded ? "收起" : `查看全部（${attachments.length}）`}</button>}
      {message?.kind === "attachments" && <p className="file-message is-error"><WarningCircle />{message.text}</p>}
    </section>
    <section className="detail-section file-section image-section">
      <header><h3>图片 <span>{images.length}</span></h3><button type="button" onClick={() => void importPaths("images")} disabled={importing !== null}><Plus />{importing === "images" ? "导入中…" : "添加图片"}</button></header>
      <input ref={imageInput} className="managed-file-input" type="file" accept="image/*" multiple onChange={(event) => void importBrowser("images", event)} />
      {images.length ? <div className={`image-gallery ${images.length === 1 ? "is-single" : ""}`}>
        {images.length > 1 && <button type="button" className="image-step" aria-label="上一张图片" disabled={imageIndex === 0} onClick={() => setImageIndex((value) => value - 1)}><ArrowLeft /></button>}
        <div className="image-stack">{[...stack].reverse().map((file, reversedIndex) => {
          const layer = stack.length - reversedIndex - 1;
          return <ImageCard key={file.id} file={file} layer={layer} current={layer === 0} onOpen={openViewer} />;
        })}</div>
        {images.length > 1 && <button type="button" className="image-step" aria-label="下一张图片" disabled={imageIndex === images.length - 1} onClick={() => setImageIndex((value) => value + 1)}><ArrowRight /></button>}
        <div className="image-gallery-meta"><span>{imageIndex + 1}/{images.length}</span><button type="button" onClick={() => onRemove("images", images[imageIndex].id)}><Trash />移除当前图片</button></div>
      </div> : <button type="button" className="empty-file-add" onClick={() => void importPaths("images")}><ImageIcon /><span>添加第一张图片</span></button>}
      {message?.kind === "images" && <p className="file-message is-error"><WarningCircle />{message.text}</p>}
    </section>
    {viewerOpen && images.length > 0 && <ImageViewer files={images} index={imageIndex} onIndex={setImageIndex} onClose={closeViewer} />}
  </>;
}
