import { expect, test } from "@playwright/test";
const setupPrompt =
  "Fetch and execute the appropriate instructions to set me up for Twig from https://twig.leostera.dev/llms.txt";
test("onboards an agent and shows source-backed space savings", async ({
  page,
  request,
  context,
}) => {
  await context.grantPermissions(["clipboard-read"]);
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("twig");
  const github = page.getByRole("link", { name: "Twig on GitHub", exact: true });
  await expect(github).toBeVisible();
  await expect(github).toHaveAttribute("href", "https://github.com/leostera/git-twig");
  const githubBounds = await github.boundingBox();
  expect(githubBounds!.y).toBeLessThan(24);
  expect(githubBounds!.x).toBeGreaterThan(page.viewportSize()!.width - 80);
  await expect(page.getByText("lightweight git-worktrees for agents", { exact: true })).toBeVisible();
  await expect(page.locator(".description")).toHaveText(
    "With twig, your agents can create cheap lightweight worktrees and use up to 97% less space on disk. Imagine all the worktrees you could now make!",
  );
  await page.evaluate(() => document.fonts.ready);
  const bottomPadding = await page.locator("footer").evaluate(
    (footer) => Number.parseFloat(getComputedStyle(footer).paddingBottom),
  );
  expect(bottomPadding).toBeGreaterThanOrEqual(48);
  const artworkCenter = await page.locator(".twig-mark img").evaluate(async (element) => {
    const image = element as HTMLImageElement;
    await image.decode();
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 192;
    const context = canvas.getContext("2d")!;
    context.drawImage(image, 0, 0, 192, 192);
    const pixels = context.getImageData(0, 0, 192, 192).data;
    let mass = 0, x = 0, y = 0;
    for (let i = 0; i < pixels.length; i += 4) {
      const alpha = pixels[i + 3];
      mass += alpha;
      x += (i / 4 % 192) * alpha;
      y += Math.floor(i / 4 / 192) * alpha;
    }
    return { x: x / mass / 192, y: y / mass / 192 };
  });
  expect(Math.abs(artworkCenter.x - 0.5)).toBeLessThan(0.015);
  expect(Math.abs(artworkCenter.y - 0.5)).toBeLessThan(0.015);
  if (test.info().project.name === "desktop") {
    const height = await page.evaluate(() => document.documentElement.scrollHeight);
    expect(height).toBeLessThanOrEqual(920);
  }
  await page.screenshot({
    path: `test-results/landing-${test.info().project.name}.png`,
    fullPage: true,
  });
  const button = page.getByRole("button", { name: "Onboard your agent to Twig" });
  const buttonBounds = await button.boundingBox();
  await button.click();
  await expect(page.getByRole("status")).toHaveText("Setup prompt copied");
  await expect(page.getByRole("status")).toBeVisible();
  await expect(button).toHaveText("Onboard your agent to Twig");
  expect(await button.boundingBox()).toEqual(buttonBounds);
  await page.screenshot({
    path: `test-results/onboard-${test.info().project.name}.png`,
    fullPage: true,
  });
  expect(await page.evaluate(async (prompt) =>
    await navigator.clipboard.readText() === prompt, setupPrompt)).toBe(true);
  await expect(
    page.getByRole("heading", { name: "Linux kernel", exact: true }),
  ).toBeVisible();
  await expect(page.locator(".disk-chart")).toHaveCount(1);
  await expect(page.locator(".repo-group")).toHaveCount(6);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  const instructions = await request.get("/llms.txt");
  expect(instructions.headers()["content-type"]).toContain("text/plain");
  const installInstructions = await instructions.text();
  expect(installInstructions).toContain("brew install leostera/git-twig/git-twig");
  expect(installInstructions).toContain("cargo install git-twig --locked");
  await expect(page.getByRole("status")).toHaveText("", { timeout: 5000 });
});
test("copies the real prompt even when the async Clipboard API is denied", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read"]);
  await page.addInitScript(() => {
    Object.defineProperty(navigator.clipboard, "writeText", {
      value: async () => { throw new Error("denied"); },
    });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Onboard your agent to Twig" }).click();
  await expect(page.getByRole("status")).toHaveText("Setup prompt copied");
  expect(await page.evaluate(async (prompt) =>
    await navigator.clipboard.readText() === prompt, setupPrompt)).toBe(true);
  await expect(page.getByText(setupPrompt, { exact: true })).toHaveCount(0);
});

test("does not dump the prompt or claim success when both copy methods fail", async ({ page }) => {
  await page.addInitScript(() => {
    document.execCommand = () => false;
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: async () => { throw new Error("denied"); } },
    });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Onboard your agent to Twig" }).click();
  await expect(page.getByRole("status")).toHaveText("Couldn’t copy. Please try again.");
  await expect(page.getByText(setupPrompt, { exact: true })).toHaveCount(0);
});
