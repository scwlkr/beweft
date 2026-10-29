import { createServer, request, type ServerResponse } from "node:http";
import { expect, test } from "@playwright/test";
import { withWorkspace } from "./helpers";

test("stalled HTTP/1 metadata bodies release sockets and archived brand recovery stays usable", async () => {
  await withWorkspace(async (page, context, write) => {
    const alpha = await write("/brands", { name: "Socket Alpha" });
    const beta = await write("/brands", { name: "Socket Beta" });
    for (const [brand, title] of [
      [alpha, "Socket Alpha draft"],
      [beta, "Socket Beta draft"],
    ])
      await write("/posts", {
        brand_id: brand.id,
        title,
        text: `${title} content`,
        media_ids: [],
        variants: [],
      });
    await context.addInitScript(
      (id) => localStorage.setItem("beweft.brand", id),
      alpha.id,
    );
    const upstream = new URL(process.env.BEWEFT_TEST_URL!);
    const held = new Set<ServerResponse>();
    let stall = false,
      starts = 0,
      brandRequests = 0;
    const proxy = createServer((incoming, outgoing) => {
      const path = incoming.url ?? "/";
      if (
        stall &&
        ["/api/v1/providers", "/api/v1/jobs/health"].includes(path)
      ) {
        starts++;
        held.add(outgoing);
        outgoing.on("close", () => held.delete(outgoing));
        outgoing.writeHead(200, {
          "Content-Type": "application/json",
          "Cache-Control": "no-store",
        });
        outgoing.flushHeaders();
        outgoing.write('{"data":'); // Send real headers and a partial body, then keep the socket open.
        return;
      }
      const brands = path === "/api/v1/brands" && incoming.method === "GET";
      if (brands) brandRequests++;
      const headers = { ...incoming.headers, host: upstream.host };
      if (headers.origin) headers.origin = upstream.origin;
      const forward = request(
        new URL(path, upstream),
        { method: incoming.method, headers },
        (response) => {
          if (brands && response.statusCode === 200) {
            const chunks: Buffer[] = [];
            response.on("data", (chunk: Buffer) => chunks.push(chunk));
            response.on("end", () => {
              const envelope = JSON.parse(Buffer.concat(chunks).toString());
              envelope.data = envelope.data.filter((brand: { id: string }) =>
                [alpha.id, beta.id].includes(brand.id),
              );
              const { "content-length": _length, ...responseHeaders } =
                response.headers;
              outgoing.writeHead(response.statusCode!, responseHeaders);
              outgoing.end(JSON.stringify(envelope));
            });
          } else {
            outgoing.writeHead(response.statusCode ?? 502, response.headers);
            response.pipe(outgoing);
          }
        },
      );
      forward.on("error", () => {
        if (!outgoing.destroyed) {
          if (!outgoing.headersSent) outgoing.writeHead(502);
          outgoing.end();
        }
      });
      incoming.on("aborted", () => forward.destroy());
      outgoing.on("close", () => {
        if (!outgoing.writableEnded) forward.destroy();
      });
      incoming.pipe(forward);
    });
    await new Promise<void>((resolve) => proxy.listen(0, "127.0.0.1", resolve));
    const address = proxy.address();
    if (!address || typeof address === "string")
      throw new Error("Proxy did not bind to loopback");
    try {
      await page.clock.install();
      await page.goto(`http://127.0.0.1:${address.port}/#drafts`);
      await expect(
        page.getByRole("heading", { name: "Socket Alpha draft" }),
      ).toBeVisible();
      stall = true;
      await page.getByRole("button", { name: "Refresh workspace" }).click();
      await expect.poll(() => starts).toBeGreaterThanOrEqual(2);
      await page.getByRole("button", { name: "Refresh workspace" }).click();
      await expect.poll(() => starts).toBeGreaterThanOrEqual(4);
      await expect.poll(() => held.size).toBe(2);
      await page.clock.fastForward(5_100);
      await expect.poll(() => held.size).toBe(0);
      await page.getByRole("button", { name: "Edit draft" }).click();
      for (let iteration = 1; iteration <= 3; iteration++) {
        await page.clock.fastForward(30_000);
        await expect
          .poll(() => starts)
          .toBeGreaterThanOrEqual(4 + iteration * 2);
        await page.clock.fastForward(5_100);
        await expect.poll(() => held.size).toBe(0);
      }
      const beforeArchiveRefresh = brandRequests;
      const archived = await context.request.patch(
        `/api/v1/brands/${alpha.id}`,
        {
          data: { archived: true },
          headers: { "X-Beweft-Request": "1", Origin: upstream.origin },
        },
      );
      expect(archived.ok(), await archived.text()).toBe(true);
      await page.clock.fastForward(30_000);
      await expect
        .poll(() => brandRequests)
        .toBeGreaterThan(beforeArchiveRefresh);
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await expect(page.getByLabel("Current brand")).toHaveValue(beta.id);
      await expect(
        page.getByRole("heading", { name: "Socket Beta draft" }),
      ).toBeVisible();
      await page.getByRole("button", { name: "Edit draft" }).click();
      await expect(page.getByLabel("Post text", { exact: true })).toHaveValue(
        "Socket Beta draft content",
      );
      await page.getByRole("button", { name: "Close dialog" }).click();
      await expect.poll(() => held.size).toBeGreaterThan(0);
      await page.getByRole("button", { name: "Sign out" }).click();
      await expect.poll(() => held.size).toBe(0);
    } finally {
      await page.goto("about:blank").catch(() => {});
      proxy.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        proxy.close((error) => (error ? reject(error) : resolve())),
      );
    }
  });
});
