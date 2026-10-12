import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { mkdirSync, readFileSync } from "node:fs";

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
  await expect(page.getByRole("combobox", { name: "Priority fee" })).toHaveValue("zero");
});

test("Prove member loading and request failure never imply an empty run", async ({ page }) => {
  // Reuse the captured report shape; the browser-only fixture performs no writes.
  const { prove_report: report } = JSON.parse(readFileSync("../data/live-prove-validation.json", "utf8"));
  report.lock.source = "sim";
  report.lock.id = "loading-check";
  report.lock.seed = "42";
  await page.route("**/v1/prove/loading-check", (route) => route.fulfill({
    contentType: "application/json", body: JSON.stringify(report),
  }));
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  await page.route("**/v1/prove/loading-check/canaries", async (route) => {
    await gate;
    await route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ code: "UNAVAILABLE", message: "Evidence unavailable" }) });
  });
  await page.goto("/#prove/loading-check");
  try {
    await expect(page.getByRole("status", { name: "Loading canary members…" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "No members yet" })).toHaveCount(0);
  } finally {
    release();
  }
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(page.getByText("Member evidence unavailable.", { exact: true })).toBeVisible();
  await expect(page.getByRole("status", { name: "Loading canary members…" })).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "No members yet" })).toHaveCount(0);
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
