import { test, expect } from "@playwright/test";

// ---------------------------------------------------------------------------
// Smoke tests — verify the app boots and core navigation works.
// ---------------------------------------------------------------------------

test.describe("Smoke", () => {
  test("app loads and shows the sidebar", async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL(/\/boards$/);
    await expect(page.locator("nav")).toBeVisible();
    await expect(page.getByText("Boards", { exact: true })).toBeVisible();
  });

  test("can navigate to Boards list", async ({ page }) => {
    await page.goto("/boards");
    await expect(page.getByRole("heading", { name: "Boards" })).toBeVisible({
      timeout: 10_000,
    });
  });

  test("can navigate to Board view", async ({ page }) => {
    await page.goto("/boards");
    await page.getByPlaceholder("e.g. release").fill("e2e-smoke");
    await page.getByRole("button", { name: "Create board" }).click();
    await expect(page).toHaveURL(/\/boards\/e2e-smoke$/);
    await expect(page.getByText("Backlog")).toBeVisible({ timeout: 10_000 });
  });

  test("can navigate to Settings view", async ({ page }) => {
    await page.goto("/settings");
    await expect(page.getByText("Settings")).toBeVisible({ timeout: 10_000 });
  });

  test("can navigate to Agents view", async ({ page }) => {
    await page.goto("/agents");
    await expect(page.getByText("Agents")).toBeVisible({ timeout: 10_000 });
  });

  test("Board shows columns", async ({ page }) => {
    await page.goto("/boards");
    await page.getByPlaceholder("e.g. release").fill("e2e-columns");
    await page.getByRole("button", { name: "Create board" }).click();
    await expect(page).toHaveURL(/\/boards\/e2e-columns$/);
    const columns = page.locator("[class*='column'], [class*='Column']");
    await expect(columns.first()).toBeVisible({ timeout: 10_000 });
  });
});
