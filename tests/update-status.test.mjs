import assert from "node:assert/strict";
import test from "node:test";
import { friendlyUpdateError } from "../src/update-status.ts";

test("download failures never claim a retained installer exists", () => {
  assert.match(friendlyUpdateError(new Error("connection reset"), "download"), /尚未生成可手动重试的安装包/);
  assert.match(friendlyUpdateError(new Error("error sending request for url https://updates.invalid/latest.json"), "download"), /下载失败/);
  assert.doesNotMatch(friendlyUpdateError(new Error("error sending request for url https://updates.invalid/latest.json"), "download"), /未配置更新源/);
  assert.doesNotMatch(friendlyUpdateError(new Error("connection reset"), "download"), /安装程序启动失败/);
});

test("signature, configuration and install failures have distinct messages", () => {
  assert.match(friendlyUpdateError(new Error("signature verification failed"), "download"), /签名验证失败/);
  assert.match(friendlyUpdateError(new Error("endpoint configuration missing"), "check"), /未配置更新源/);
  assert.match(friendlyUpdateError(new Error("Error opening file for writing"), "install"), /仍被占用/);
});

test("highest stable release failures retain actionable details and never claim latest", () => {
  for (const detail of [
    "版本检查失败：GitHub 请求已限流，请稍后重试。",
    "版本检查失败：最高正式版本 0.2.10 缺少 latest.json 更新清单。",
    "版本检查失败：最高正式版本 0.2.10 的更新清单版本不匹配。",
    "版本检查失败：最高正式版本 0.2.10 的更新包缺失或来源不匹配。",
  ]) {
    assert.equal(friendlyUpdateError(detail, "check"), detail);
    assert.equal(friendlyUpdateError(new Error(detail), "check"), detail);
    assert.doesNotMatch(friendlyUpdateError(detail, "check"), /已经是最新/);
  }
});

test("development checks explain channel isolation instead of reporting a network failure", () => {
  const detail = "当前构建未配置更新源；开发版不会访问生产更新服务，请在日常安装版检查更新。";
  assert.equal(friendlyUpdateError(detail, "check"), detail);
  assert.doesNotMatch(friendlyUpdateError(detail, "check"), /网络连接/);
});
