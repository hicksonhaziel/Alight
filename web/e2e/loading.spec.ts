import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { mkdirSync } from "node:fs";

test("initial skeleton is accessible, responsive, motion-aware and replaced by real Sim data", async ({ page }) => {
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  await page.route("**/v1/health", async (route) => {
    await gate;
    await route.continue();
  });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/", { waitUntil: "domcontentloaded" });
  try {
    await expect(page.getByRole("status", { name: "Connecting to the observatory…" })).toBeVisible();
    await expect(page.locator(".page-skeleton")).toHaveAttribute("aria-busy", "true");
    await expect(page.locator(".skeleton").first()).toHaveCSS("animation-name", "none");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
    mkdirSync("../.alight/hardening", { recursive: true });
    await page.screenshot({ path: "../.alight/hardening/skeleton-loading.png", fullPage: true });
  } finally {
    release();
  }
  await expect(page.getByRole("heading", { name: "Network overview" })).toBeVisible();
  await expect(page.locator(".topbar .source-badge")).toHaveText("SIMULATED");
  await expect(page.locator(".page-skeleton")).toHaveCount(0);
});

test("tape loading never flashes empty evidence and a failed request ends the skeleton", async ({ page }) => {
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  await page.route("**/v1/tape?*", async (route) => {
    await gate;
    await route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ code: "UNAVAILABLE", message: "Evidence unavailable" }) });
  });
  await page.goto("/#tape");
  try {
    await expect(page.getByRole("heading", { name: "Tape & receipts" })).toBeVisible();
    await expect(page.locator(".skeleton-chart")).toBeVisible();
    await expect(page.locator(".skeleton-table")).toBeVisible();
    await expect(page.getByRole("heading", { name: "No transfer records" })).toHaveCount(0);
    await expect(page.getByRole("heading", { name: "No paid tips in this window" })).toHaveCount(0);
  } finally {
    release();
  }
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(page.locator(".skeleton-content")).toHaveCount(0);
  await expect(page.getByText("Transfer evidence unavailable.", { exact: true })).toBeVisible();
});
