import { test, expect } from "@playwright/test";

test("dataset deep links open the existing downloads section without adding a page", async ({
  page,
}) => {
  await page.goto("/#ledger");
  await expect(
    page.getByRole("heading", { name: "Forecast ledger", exact: true }),
  ).toBeVisible();
  const downloads = page.locator(".page-content details.panel");
  await expect(downloads).not.toHaveAttribute("open");
  await page.getByRole("link", { name: "Download data", exact: true }).click();
  await expect(page).toHaveURL(/#datasets$/);
  await expect(downloads).toHaveAttribute("open", "");
  await expect(
    page.getByRole("link", {
      name: "Dataset schema and verification",
      exact: true,
    }),
  ).toBeVisible();
  await expect(downloads.locator("summary")).toBeInViewport();
  await expect(
    page.locator('.desktop-sidebar nav a[href="#ledger"]'),
  ).toHaveAttribute("aria-current", "page");
  await page.locator('.desktop-sidebar nav a[href="#ledger"]').click();
  await expect(downloads).not.toHaveAttribute("open");
  await page.goto("/#datasets");
  await page.reload();
  await expect(downloads).toHaveAttribute("open", "");
  await expect(downloads.locator("summary")).toBeInViewport();
});

test("changing quote settings invalidates an in-flight preview before it becomes lockable", async ({
  page,
}) => {
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  let captured!: () => void;
  const waiting = new Promise<void>((resolve) => {
    captured = resolve;
  });
  await page.route(
    "**/v1/quote?*",
    async (route) => {
      const response = await route.fetch();
      captured();
      await gate;
      await route.fulfill({ response });
    },
    { times: 1 },
  );
  await page.goto("/#quote");
  await page
    .getByRole("button", { name: "Compute quote", exact: true })
    .click();
  try {
    await waiting;
    await page
      .getByRole("combobox", { name: "Horizon", exact: true })
      .selectOption("2");
  } finally {
    release();
  }
  await expect(
    page.getByRole("button", { name: "Compute quote", exact: true }),
  ).toBeEnabled();
  await expect(page.locator(".quote-probability")).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Connect operator to lock", exact: true }),
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "Compute quote", exact: true })
    .click();
  await expect(page.locator(".quote-probability")).toContainText(
    "within 2 slots",
  );
});
