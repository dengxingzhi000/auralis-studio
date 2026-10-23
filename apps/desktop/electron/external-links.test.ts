import { describe, expect, it } from "vitest";

import { isOpenableExternalUrl } from "./external-links";

describe("isOpenableExternalUrl", () => {
  it("放行 http 与 https", () => {
    expect(isOpenableExternalUrl("https://example.com/")).toBe(true);
    expect(isOpenableExternalUrl("http://example.com/")).toBe(true);
    expect(isOpenableExternalUrl("HTTPS://EXAMPLE.COM/")).toBe(true);
  });

  it("拒绝能拉起本机程序的协议", () => {
    // 这些是最要紧的：交给系统就等于「一条数据启动一个程序」。
    expect(isOpenableExternalUrl("file:///C:/Windows/System32/calc.exe")).toBe(false);
    expect(isOpenableExternalUrl("ms-msdt://id")).toBe(false);
    expect(isOpenableExternalUrl("smb://example.com/share")).toBe(false);
    expect(isOpenableExternalUrl("vbscript:msgbox(1)")).toBe(false);
    expect(isOpenableExternalUrl("javascript:alert(1)")).toBe(false);
    expect(isOpenableExternalUrl("data:text/html,<script>1</script>")).toBe(false);
  });

  it("拒绝没有协议的字符串", () => {
    expect(isOpenableExternalUrl("")).toBe(false);
    expect(isOpenableExternalUrl("example.com")).toBe(false);
    expect(isOpenableExternalUrl("//example.com")).toBe(false);
  });

  it("拒绝看起来像 http 但其实不是的", () => {
    // "httpsomething://" 不是 https；"https:/" 只有一个斜杠。
    expect(isOpenableExternalUrl("httpsomething://example.com")).toBe(false);
    expect(isOpenableExternalUrl("https:/example.com")).toBe(false);
    // 协议后面必须是 "//"
    expect(isOpenableExternalUrl("https:example.com")).toBe(false);
  });

  it("拒绝以空白开头的伪协议", () => {
    expect(isOpenableExternalUrl(" file:///C:/Windows/System32/calc.exe")).toBe(false);
  });
});
