import { test, expect } from "@playwright/test";

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
