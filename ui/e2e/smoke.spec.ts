import { test, expect } from "@playwright/test";

// ---------------------------------------------------------------------------
// Smoke tests — verify the app boots and core navigation works.
// ---------------------------------------------------------------------------

test.describe("Smoke", () => {
  test("app loads and shows the sidebar", async ({ page }) => {
    await page.goto("/");
    // The sidebar contains the Boards nav link.
    await expect(page.locator("nav")).toBeVisible();
    await expect(page.getByText("Boards")).toBeVisible();
  });

  test("can navigate to Board view", async ({ page }) => {
    await page.goto("/board");
    // Board view should render column headers.
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
    await page.goto("/board");
    // Default pipeline stages should render as columns.
    const columns = page.locator("[class*='column'], [class*='Column']");
    await expect(columns.first()).toBeVisible({ timeout: 10_000 });
  });
});
