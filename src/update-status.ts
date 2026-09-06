export type UpdateFailureStage = "check" | "download" | "install";

function errorDetail(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export function friendlyUpdateError(error: unknown, stage: UpdateFailureStage) {
  const detail = errorDetail(error).trim();
  if (/no.*endpoint|endpoint.*(?:empty|invalid|not configured|missing)|configuration.*(?:missing|invalid)|update.*not configured|pubkey.*(?:empty|missing)/i.test(detail)) {
    return "当前构建未配置更新源。开发版请重新构建，日常使用请在安装版检查更新。";
  }
  if (stage === "download") {
    if (/signature|verify|public key|minisign|验签/i.test(detail)) {
      return "更新包下载完成，但签名验证失败；未启动安装，也没有可重试的安装包。";
    }
    if (/space|disk|write|permission|denied|空间|磁盘/i.test(detail)) {
      return "更新包下载失败：无法写入临时存储，请检查磁盘空间和目录权限。";
    }
    return "更新包下载失败，请检查网络后重试；尚未生成可手动重试的安装包。";
  }
  if (stage === "install") {
    if (/opening file|in use|used by another process|占用|process/i.test(detail)) {
      return "安装未能开始：目标 TodoList 组件仍被占用，旧版本没有被部分覆盖。";
    }
    return detail
      ? `安装程序启动失败：${detail}`
      : "安装程序启动失败；如果安装包已生成，TodoList 会保留它并显示重试入口。";
  }
  return "检查更新失败，请确认网络连接和更新服务状态后重试。";
}
