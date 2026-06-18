import { describe, it, expect, afterEach } from "vitest";
import { mkdtempSync, rmSync, writeFileSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  FileAuditLog,
  MemoryAuditLog,
  chainHash,
  canonicalize,
  GENESIS_HASH,
  type AuditEntry,
} from "./audit-log.js";

const dirs: string[] = [];
function tmpFile(name: string): string {
  const d = mkdtempSync(join(tmpdir(), "cadence-audit-"));
  dirs.push(d);
  return join(d, name);
}
afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

const entry = (event: string, tsMs = 1): AuditEntry => ({ event, detail: { a: 1 }, tsMs });

describe("canonicalize", () => {
  it("sorts keys and stringifies bigints deterministically", () => {
    const a = canonicalize({ b: 2n, a: 1 });
    const b = canonicalize({ a: 1, b: 2n });
    expect(a).toBe(b);
    expect(a).toContain('"b":"2"');
  });
});

describe("chainHash", () => {
  it("links to prevHash so any reorder changes the hash", () => {
    const h1 = chainHash(GENESIS_HASH, entry("x"));
    const h2 = chainHash("deadbeef", entry("x"));
    expect(h1).not.toBe(h2);
    expect(h1).toMatch(/^[0-9a-f]{64}$/);
  });
});

describe("MemoryAuditLog", () => {
  it("chains entries and verifies intact", async () => {
    const log = new MemoryAuditLog();
    const h1 = await log.record(entry("proposed"));
    const h2 = await log.record(entry("slice_submitted"));
    expect(h1).not.toBe(h2);
    expect(await log.verify()).toBe(true);
    const entries = log.entries();
    expect(entries[0]?.entry.prevHash).toBe(GENESIS_HASH);
    expect(entries[1]?.entry.prevHash).toBe(h1);
  });
});

describe("FileAuditLog", () => {
  it("persists a hash-chained JSONL file and verifies it", async () => {
    const path = tmpFile("audit.jsonl");
    const log = new FileAuditLog(path);
    await log.record(entry("proposed"));
    await log.record(entry("fill_recorded"));
    await log.flush();
    const lines = readFileSync(path, "utf8").trim().split("\n");
    expect(lines).toHaveLength(2);
    expect(await log.verify()).toBe(true);
  });

  it("continues the same chain across a restart", async () => {
    const path = tmpFile("audit.jsonl");
    const first = new FileAuditLog(path);
    const h1 = await first.record(entry("proposed"));
    await first.flush();

    const second = new FileAuditLog(path);
    await second.init();
    await second.record(entry("attested"));
    await second.flush();

    const lines = readFileSync(path, "utf8").trim().split("\n");
    expect(lines).toHaveLength(2);
    const rec2 = JSON.parse(lines[1] ?? "{}");
    expect(rec2.entry.prevHash).toBe(h1);
    expect(await second.verify()).toBe(true);
  });

  it("detects tampering — an edited entry breaks verify()", async () => {
    const path = tmpFile("audit.jsonl");
    const log = new FileAuditLog(path);
    await log.record(entry("proposed"));
    await log.record(entry("fill_recorded"));
    await log.flush();

    const lines = readFileSync(path, "utf8").trim().split("\n");
    const rec0 = JSON.parse(lines[0] ?? "{}");
    rec0.entry.detail = { a: 999 }; // tamper without recomputing the hash
    writeFileSync(path, `${JSON.stringify(rec0)}\n${lines[1]}\n`, "utf8");

    const reopened = new FileAuditLog(path);
    expect(await reopened.verify()).toBe(false);
  });

  it("verify() is true for an absent file (empty chain)", async () => {
    const log = new FileAuditLog(tmpFile("missing.jsonl"));
    expect(await log.verify()).toBe(true);
  });
});
