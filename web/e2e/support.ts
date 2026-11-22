import { mkdirSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, type Page } from "@playwright/test";

type Session = { home: string; port: number; token: string; url: string };

export function session(): Session {
  const file = join(dirname(fileURLToPath(import.meta.url)), "..", ".e2e-session.json");
  return JSON.parse(readFileSync(file, "utf8")) as Session;
}

export async function api(
  path: string,
  options: { method?: string; body?: unknown; set?: string } = {},
): Promise<Record<string, unknown>> {
  const current = session();
  const headers: Record<string, string> = { Authorization: `Bearer ${current.token}` };
  if (options.set) headers["X-Boggled-Set"] = options.set;
  if (options.body !== undefined) headers["Content-Type"] = "application/json";
  const response = await fetch(`http://127.0.0.1:${current.port}${path}`, {
    method: options.method ?? "GET",
    headers,
    body: options.body !== undefined ? JSON.stringify(options.body) : undefined,
  });
  const text = await response.text();
  const body = text ? (JSON.parse(text) as Record<string, unknown>) : {};
  if (!response.ok) {
    const message = typeof body.error === "string" ? body.error : text;
    throw new Error(`${response.status} ${message}`);
  }
  return body;
}

export function tempDir(): string {
  return mkdtempSync(join(tmpdir(), "boggled-ui-"));
}

export async function initSet(name: string, code: string): Promise<string> {
  const path = join(tempDir(), "set");
  const body = await api("/api/sets/init", {
    method: "POST",
    body: { path, name, code },
  });
  return String(body.path);
}

export async function createCard(set: string, fields: Record<string, string>): Promise<void> {
  await api("/api/cards", { method: "POST", set, body: fields });
}

export function cardNames(setPath: string): string[] {
  const dir = join(setPath, "cards");
  return readdirSync(dir)
    .filter((name) => name.endsWith(".json"))
    .map((name) => JSON.parse(readFileSync(join(dir, name), "utf8")) as { name?: string })
    .map((card) => card.name ?? "");
}

export function readCards(setPath: string): Array<Record<string, unknown>> {
  const dir = join(setPath, "cards");
  return readdirSync(dir)
    .filter((name) => name.endsWith(".json"))
    .map((name) => JSON.parse(readFileSync(join(dir, name), "utf8")) as Record<string, unknown>);
}

export function writeInstant(setPath: string): void {
  const uuid = crypto.randomUUID();
  const dir = join(setPath, "cards");
  mkdirSync(dir, { recursive: true });
  writeFileSync(
    join(dir, `${uuid}.json`),
    JSON.stringify({
      uuid,
      name: "Zap",
      layout: "normal",
      type: "Instant",
      types: ["Instant"],
      manaCost: "{R}",
      text: "Deal 1 damage.",
      rarity: "common",
      number: "9",
    }),
  );
}

export const TINY_PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64",
);

export function installToken(page: Page): Promise<void> {
  const current = session();
  return page.addInitScript((token) => {
    window.__BOGGLED__ = { token, url: "" };
  }, current.token);
}

export async function openSet(page: Page, setPath: string, title: string): Promise<void> {
  await page.goto("/");
  await page.getByRole("textbox", { name: "Set folder", exact: true }).fill(setPath);
  await page.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("heading", { name: title, exact: true })).toBeVisible();
}

function crc32(buf: Buffer): number {
  let crc = 0xffffffff;
  for (let index = 0; index < buf.length; index += 1) {
    crc ^= buf[index];
    for (let bit = 0; bit < 8; bit += 1) {
      const lsb = crc & 1;
      crc = (crc >>> 1) ^ (lsb ? 0xedb88320 : 0);
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

export function zipStore(files: Array<{ name: string; data: Buffer }>): Buffer {
  const parts: Buffer[] = [];
  const central: Buffer[] = [];
  let offset = 0;
  for (const file of files) {
    const name = Buffer.from(file.name);
    const crc = crc32(file.data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(name.length, 26);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(file.data.length, 18);
    local.writeUInt32LE(file.data.length, 22);
    parts.push(local, name, file.data);
    const entry = Buffer.alloc(46);
    entry.writeUInt32LE(0x02014b50, 0);
    entry.writeUInt16LE(20, 4);
    entry.writeUInt16LE(20, 6);
    entry.writeUInt32LE(crc, 16);
    entry.writeUInt32LE(file.data.length, 20);
    entry.writeUInt32LE(file.data.length, 24);
    entry.writeUInt16LE(name.length, 28);
    entry.writeUInt32LE(offset, 42);
    central.push(entry, name);
    offset += local.length + name.length + file.data.length;
  }
  const directory = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(files.length, 8);
  end.writeUInt16LE(files.length, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...parts, directory, end]);
}

export const SAMPLE_SET = `mse version: 2.0.2
game: magic
stylesheet: magic-m15-altered.mse-style
set info:
\ttitle: Steam Test
\tset code: stm
\tdescription: Gears and birds
card:
\tname: Brass Owl
\tcasting cost: 2U
\ttype: Artifact Creature - Bird
\trule text: Flying
\tpower: 1
\ttoughness: 3
\trarity: uncommon
`;
