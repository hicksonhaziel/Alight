import { defineConfig } from "@playwright/test";
import { fileURLToPath } from "node:url";
export default defineConfig({
  testDir: "./e2e",
  timeout: 60000,
  workers: 1,
  fullyParallel: false,
  reporter: [["list"], ["json", { outputFile: "../.alight/phase4/e2e.json" }]],
  outputDir: "../.alight/phase4/browser-results",
  use: {
    baseURL: "http://127.0.0.1:5180",
    viewport: { width: 1440, height: 1000 },
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
    launchOptions: process.env.ALIGHT_BROWSER_EXECUTABLE
      ? { executablePath: process.env.ALIGHT_BROWSER_EXECUTABLE }
      : {},
  },
  webServer: [
    {
      command: "python3 scripts/serve_workbench_test.py",
      cwd: fileURLToPath(new URL("..", import.meta.url)),
      url: "http://127.0.0.1:8082/v1/health",
      timeout: 60000,
      reuseExistingServer: false,
    },
    {
      command: "npm run dev -- --port 5180",
      url: "http://127.0.0.1:5180",
      env: { ALIGHT_API_ORIGIN: "http://127.0.0.1:8082" },
      timeout: 60000,
      reuseExistingServer: false,
    },
  ],
});
