import { invoke } from "@tauri-apps/api/core";

function isTauriRuntime() {
  return "__TAURI_INTERNALS__" in window;
}

export async function openStickyWindow(minimizeMain = false) {
  if (isTauriRuntime()) {
    await invoke("open_sticky_window", { minimizeMain });
    return;
  }
  const sticky = window.open("/?mode=sticky", "todolist-sticky", "popup,width=370,height=470");
  if (!sticky) throw new Error("浏览器阻止了打开便签，请允许此站点弹出窗口后重试。");
}

export async function openMainWindow() {
  if (isTauriRuntime()) {
    await invoke("open_main_window");
    return;
  }
  if (window.opener && !window.opener.closed) {
    window.opener.focus();
    return;
  }
  const main = window.open("/", "todolist-main");
  if (!main) throw new Error("浏览器阻止了打开任务台，请允许此站点弹出窗口后重试。");
  main.focus();
}

export async function closeCurrentWindow() {
  if (isTauriRuntime()) {
    const { getCurrentWebviewWindow } = await import("@tauri-apps/api/webviewWindow");
    await getCurrentWebviewWindow().close();
    return;
  }
  window.close();
}
