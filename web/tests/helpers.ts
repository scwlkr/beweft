import {
  expect,
  chromium,
  type Page,
  type BrowserContext,
} from "@playwright/test";
export function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
export async function withWorkspace(
  run: (
    page: Page,
    context: BrowserContext,
    write: (path: string, body: unknown) => Promise<any>,
  ) => Promise<void>,
) {
  const origin = process.env.BEWEFT_TEST_URL;
  if (process.env.BEWEFT_TEST_ISOLATED !== "1" || !origin)
    throw new Error("Requires disposable harness");
  const browser = await chromium.launch({ headless: true });
  try {
    const context = await browser.newContext({
      baseURL: origin,
      viewport: { width: 1440, height: 1000 },
    });
    const write = async (path: string, body: unknown) => {
      const response = await context.request.post(`${origin}/api/v1${path}`, {
        data: body,
        headers: { "X-Beweft-Request": "1", Origin: origin },
      });
      expect(response.ok(), await response.text()).toBe(true);
      return (await response.json()).data;
    };
    await write("/auth/login", {
      email: process.env.BEWEFT_TEST_EMAIL,
      password: process.env.BEWEFT_TEST_PASSWORD,
    });
    const page = await context.newPage();
    page.setDefaultTimeout(10000);
    await run(page, context, write);
  } finally {
    await browser.close();
  }
}
