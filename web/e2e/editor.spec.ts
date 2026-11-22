import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import {
  SAMPLE_SET,
  TINY_PNG,
  cardNames,
  createCard,
  initSet,
  installToken,
  openSet,
  readCards,
  tempDir,
  writeInstant,
  zipStore,
} from "./support";

test.beforeEach(async ({ page }) => {
  await installToken(page);
});

test("opens a set by typing its folder", async ({ page }) => {
  const setPath = await initSet("Lantern", "LNT");
  await openSet(page, setPath, "Lantern");
  await expect(page.getByText("0 selected")).toBeVisible();
  await expect(page.getByRole("tab", { name: "Cards" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Set info" })).toBeVisible();
});

test("positions editable fields from the Windows MSE preview", async ({ page }) => {
  test.skip(process.platform !== "win32" || !process.env.BSS_MSE_REFERENCE, "needs the pinned Windows engine");
  const set = await initSet("MSE Geometry", "GEO");
  await createCard(set, { name: "Lantern Guard", type: "Creature — Soldier", manaCost: "{1}{W}", text: "Pay {W}: Draw a card." });
  await openSet(page, set, "MSE Geometry");
  const response = page.waitForResponse(r => r.url().endsWith("/picture") && r.ok());
  await page.getByRole("row", { name: /Lantern Guard/ }).click();
  const layout = JSON.parse((await response).headers()["x-boggled-layout"]);
  const preview = page.locator(".card-face.native-layout");
  const name = preview.getByRole("textbox", { name: "Name", exact: true });
  await expect.poll(() => name.evaluate(e => e.offsetLeft / e.parentElement!.clientWidth)).toBeCloseTo(layout.name[0] / 375, 2);
  await expect.poll(() => preview.locator("img").evaluate(img => img.naturalWidth)).toBe(750);
  await name.fill("Lantern Captain");
  await name.press("Tab");
  await expect(page.getByRole("row", { name: /Lantern Captain/ })).toBeVisible();
  await expect(preview.getByRole("img", { name: "Rendered Lantern Captain" })).toBeVisible();
});

test("edits a creature and updates the grid and preview", async ({ page }) => {
  const setPath = await initSet("Lantern", "LNT");
  await createCard(setPath, { name: "Brass Owl", type: "Creature" });
  await openSet(page, setPath, "Lantern");
  await page.getByRole("row", { name: /Brass Owl/ }).click();
  await page.getByRole("textbox", { name: "Name", exact: true }).fill("Copper Owl");
  await page.getByRole("tab", { name: "Cards" }).click();
  await expect(page.getByRole("row", { name: /Copper Owl/ })).toBeVisible();
  await expect(page.getByRole("row", { name: /Brass Owl/ })).toHaveCount(0);
  const preview = page.getByRole("img", { name: "Rendered Copper Owl" });
  await expect(preview).toBeVisible();
  await expect.poll(() => preview.evaluate((img: HTMLImageElement) => img.naturalWidth)).toBeGreaterThan(0);
  expect(cardNames(setPath)).toContain("Copper Owl");
});

test("creates a creature", async ({ page }) => {
  const setPath = await initSet("Empty", "EMP");
  await openSet(page, setPath, "Empty");
  await page.getByRole("button", { name: "New card" }).click();
  await expect(page.getByRole("row", { name: /New Creature/ })).toBeVisible();
  await expect(page.getByRole("textbox", { name: "Name", exact: true })).toHaveValue("New Creature");
  expect(cardNames(setPath)).toContain("New Creature");
});

test("deletes a card", async ({ page }) => {
  const setPath = await initSet("Lantern", "LNT");
  await createCard(setPath, { name: "Brass Owl", type: "Creature" });
  await openSet(page, setPath, "Lantern");
  await page.getByRole("row", { name: /Brass Owl/ }).click();
  await page.getByRole("button", { name: "Delete card" }).click();
  await expect(page.getByRole("row", { name: /Brass Owl/ })).toHaveCount(0);
  expect(cardNames(setPath)).not.toContain("Brass Owl");
});

test("sets an illustration from a path", async ({ page }) => {
  const setPath = await initSet("Lantern", "LNT");
  await createCard(setPath, { name: "Brass Owl", type: "Creature" });
  const art = join(tempDir(), "owl.png");
  writeFileSync(art, TINY_PNG);
  await openSet(page, setPath, "Lantern");
  await page.getByRole("row", { name: /Brass Owl/ }).click();
  await expect(page.getByRole("button", { name: "Set illustration" })).toBeEnabled();
  await page.getByRole("button", { name: "Set illustration" }).click();
  await page.getByRole("textbox", { name: "Illustration" }).fill(art);
  await page.getByRole("button", { name: "Use illustration" }).click();
  await expect(page.getByRole("status")).toContainText("Illustration set");
  const card = readCards(setPath).find((face) => face.name === "Brass Owl");
  const artFile = (card?.boggled as { artFile?: string } | undefined)?.artFile ?? "";
  expect(artFile).toMatch(/^art\/.+\.png$/);
  expect(readFileSync(join(setPath, ...artFile.split("/")))).toEqual(TINY_PNG);
});

test("edits the set name and design brief", async ({ page }) => {
  const setPath = await initSet("Lantern", "LNT");
  await openSet(page, setPath, "Lantern");
  await page.getByRole("tab", { name: "Set info" }).click();
  await page.getByRole("textbox", { name: "Design brief" }).fill("Lights in the gear");
  await page.getByRole("textbox", { name: "Set name" }).fill("Lanterns");
  await page.getByRole("tab", { name: "Cards" }).click();
  await expect(page.getByRole("heading", { name: "Lanterns", exact: true })).toBeVisible();
  const document = JSON.parse(readFileSync(join(setPath, "set.json"), "utf8")) as {
    name: string;
    boggled: { designBrief: string };
  };
  expect(document.name).toBe("Lanterns");
  expect(document.boggled.designBrief).toBe("Lights in the gear");
});

test("imports an mse-set package", async ({ page }) => {
  const dir = tempDir();
  const source = join(dir, "sample.mse-set");
  const dest = join(dir, "Steam");
  writeFileSync(source, zipStore([{ name: "set", data: Buffer.from(SAMPLE_SET) }]));
  await page.goto("/");
  await page.getByRole("textbox", { name: "MSE package" }).fill(source);
  await page.getByRole("textbox", { name: "Import destination" }).fill(dest);
  await page.getByRole("button", { name: "Import", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Steam Test", exact: true })).toBeVisible();
  await expect(page.getByRole("row", { name: /Brass Owl/ })).toBeVisible();
  expect(cardNames(dest)).toContain("Brass Owl");
});

test("filters the grid with search", async ({ page }) => {
  const setPath = await initSet("Lantern", "LNT");
  await createCard(setPath, { name: "Brass Owl", type: "Creature" });
  await createCard(setPath, { name: "Copper Bear", type: "Creature" });
  await openSet(page, setPath, "Lantern");
  await page.getByRole("textbox", { name: "Search" }).fill("owl");
  await expect(page.getByRole("row", { name: /Brass Owl/ })).toBeVisible();
  await expect(page.getByRole("row", { name: /Copper Bear/ })).toHaveCount(0);
});

test("edits the boxes on the card", async ({ page }) => {
  const setPath = await initSet("Lantern", "LNT");
  await createCard(setPath, { name: "Brass Owl", type: "Creature" });
  await openSet(page, setPath, "Lantern");
  await page.getByRole("row", { name: /Brass Owl/ }).click();
  await page.getByRole("textbox", { name: "Mana cost", exact: true }).fill("{2}{G}");
  await page.getByRole("textbox", { name: "Type", exact: true }).fill("Creature — Sol");
  await expect(page.getByRole("option", { name: "Soldier", exact: true })).toBeVisible();
  await page.getByRole("option", { name: "Soldier", exact: true }).click();
  await expect(page.getByRole("textbox", { name: "Type", exact: true })).toHaveValue("Creature — Soldier");
  await page.getByRole("textbox", { name: "Rules", exact: true }).fill("Reach");
  await page.getByRole("textbox", { name: "Flavor", exact: true }).fill("The elk keeps the path.");
  await page.getByRole("textbox", { name: "Power", exact: true }).fill("3");
  await page.getByRole("textbox", { name: "Toughness", exact: true }).fill("3");
  await page.getByRole("tab", { name: "Cards" }).click();
  await expect(page.getByRole("row", { name: /Brass Owl/ })).toContainText("3/3");
  const card = readCards(setPath).find((face) => face.name === "Brass Owl");
  expect(card?.manaCost).toBe("{2}{G}");
  expect(card?.type).toBe("Creature — Soldier");
  expect(card?.text).toBe("Reach");
  expect(card?.flavorText).toBe("The elk keeps the path.");
  expect(card?.power).toBe("3");
  expect(card?.toughness).toBe("3");
});

test("shows an unsupported frame as read-only", async ({ page }) => {
  const setPath = await initSet("Lantern", "LNT");
  writeInstant(setPath);
  await openSet(page, setPath, "Lantern");
  await page.getByRole("row", { name: /Zap/ }).click();
  await expect(page.getByText("This frame is unsupported")).toBeVisible();
  await expect(page.getByRole("textbox", { name: "Name", exact: true })).toBeDisabled();
  await expect(page.getByRole("textbox", { name: "Name", exact: true })).toHaveValue("Zap");
});

test("exports an mtgjson file", async ({ page }) => {
  const setPath = await initSet("Lantern", "LNT");
  await createCard(setPath, { name: "Brass Owl", type: "Creature" });
  await openSet(page, setPath, "Lantern");
  await page.getByRole("button", { name: "Export", exact: true }).click();
  await page.getByRole("button", { name: "Export set" }).click();
  const status = page.getByRole("status");
  await expect(status).toContainText("Exported");
  const exported = (await status.innerText()).replace(/^Exported\s*/, "").trim();
  const body = readFileSync(exported, "utf8");
  expect(body).toContain("Brass Owl");
  expect(body).not.toContain("designNotes");
});

test("undoes and redoes a saved edit", async ({ page }) => {
  const setPath = await initSet("Lantern", "LNT");
  await createCard(setPath, { name: "Brass Owl", type: "Creature" });
  await openSet(page, setPath, "Lantern");
  await page.getByRole("row", { name: /Brass Owl/ }).click();
  await page.getByRole("textbox", { name: "Name", exact: true }).fill("Copper Owl");
  await page.getByRole("tab", { name: "Cards" }).click();
  await expect(page.getByRole("row", { name: /Copper Owl/ })).toBeVisible();
  await page.getByRole("tab", { name: "Cards" }).focus();
  await page.keyboard.press("Control+Z");
  await expect(page.getByRole("row", { name: /Brass Owl/ })).toBeVisible();
  expect(cardNames(setPath)).toContain("Brass Owl");
  expect(cardNames(setPath)).not.toContain("Copper Owl");
  await page.keyboard.press("Control+Y");
  await expect(page.getByRole("row", { name: /Copper Owl/ })).toBeVisible();
  expect(cardNames(setPath)).toContain("Copper Owl");
});

test("runs a command in the terminal", async ({ page }) => {
  const setPath = await initSet("Shell", "SHL");
  await openSet(page, setPath, "Shell");
  const terminal = page.getByRole("region", { name: "Terminal" });
  await terminal.click();
  await expect
    .poll(async () => terminal.evaluate((node) => node.textContent ?? ""), { timeout: 20_000 })
    .toContain(">");
  await page.keyboard.type("echo hello-boggled");
  await page.keyboard.press("Enter");
  await expect
    .poll(
      async () => {
        const text = await terminal.evaluate((node) => node.textContent ?? "");
        return text.split("hello-boggled").length - 1;
      },
      { timeout: 20_000 },
    )
    .toBeGreaterThan(1);
});
