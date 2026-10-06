import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import type { DiagnosticsPage } from "../../sdk/ts/src/types";
test("persisted synthetic changes, missing signal evidence, observer limits and fidelity remain inspectable", async ({
  page,
  request,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const response = await request.get("/v1/diagnostics");
  expect(response.ok()).toBeTruthy();
  const d = (await response.json()) as DiagnosticsPage;
  expect(d.source).toBe("sim");
  expect(d.regimes).toHaveLength(2);
  expect(
    d.regimes.every(
      (r) =>
        r.origin === "simulation" &&
        r.old_effective_n_cap === 0 &&
        r.exploration_fraction === 1,
    ),
  ).toBeTruthy();
  expect(d.observers).toHaveLength(0);
  expect(d.fidelity.comparisons).toHaveLength(0);
  await page.goto("/#regimes");
  await expect(
    page.getByRole("heading", { name: "Detected changes", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText(
      "All signal detections on this page are synthetic simulation evidence.",
      { exact: false },
    ),
  ).toBeVisible();
  await expect(
    page.locator(".timeline .source-badge").filter({ hasText: "SIMULATION" }),
  ).toHaveCount(2);
  await expect(
    page.getByRole("img", { name: "Mean slot interval over recorded windows" }),
  ).toBeVisible();
  await page
    .getByLabel("Signal", { exact: true })
    .selectOption("block_fullness");
  await expect(
    page.getByRole("heading", { name: "No measured signal in this window" }),
  ).toBeVisible();
  await page.getByText("Window values & provenance", { exact: true }).click();
  await expect(
    page.getByText("no_provider_block_sample_in_sim").first(),
  ).toBeVisible();
  expect(
    (
      await new AxeBuilder({ page })
        .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
        .analyze()
    ).violations,
  ).toEqual([]);
  await page
    .getByLabel("Signal", { exact: true })
    .selectOption("reference_landing_rate");
  await expect(
    page.getByRole("img", {
      name: "Reference landing rate over recorded windows",
    }),
  ).toBeVisible();
  await page.getByText("Window values & provenance", { exact: true }).click();
  await page.screenshot({
    path: "../.alight/phase5/regimes-dark.png",
    fullPage: true,
  });
  await page.goto("/#health");
  await expect(
    page.getByRole("heading", { name: "Observer comparison", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "No comparable observer population" }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Alert history", exact: true }),
  ).toBeVisible();
  await expect(
    page
      .getByText(
        "Measured signal change: old evidence reset and bounded exploration enabled",
      )
      .first(),
  ).toBeVisible();
  await page.screenshot({
    path: "../.alight/phase5/health-dark.png",
    fullPage: true,
  });
  await page.goto("/#tape");
  await expect(
    page.getByRole("heading", { name: "Workload fidelity", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "No matched position evidence" }),
  ).toBeVisible();
  await expect(
    page.getByText(
      "Raw index is descriptive, not normalized block rank or swap performance; passive transfers never train the model",
    ),
  ).toBeVisible();
  expect(errors).toEqual([]);
});
