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
