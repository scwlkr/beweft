import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  workers: 1,
  timeout: 90000,
  reporter: "list",
  outputDir: process.env.BEWEFT_TEST_OUTPUT ?? "../.local/playwright-results",
  use: {
    baseURL: process.env.BEWEFT_TEST_URL,
    trace: "off",
    screenshot: "off",
    video: "off",
  },
});
