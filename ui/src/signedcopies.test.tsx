// @vitest-environment jsdom
// The Signed copies page on the app's real route table: each chosen file is uploaded, a failed one can be tried
// again, and the sheet has the header and one row per uploaded file with its path.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { createMemoryRouter, RouterProvider } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setFrameMode } from "@ferrum/records/frame";
import { CONTRACTS_PACKAGES, routes } from "./routes";
import { COLUMNS } from "./SignedCopies";

const me = {
  accounts: null, default_company: null, display_timezone: "UTC", fiscal_year_start_month: 1, packages: ["core", "sign", "contracts"], scopes: {},
  shell_v2: true, tenant: "t1", timezone: "UTC", today: "2026-11-18", two_factor_setup: false,
  user: { email: "legal@example.test", full_name: "Lee Gal", id: "u1", is_admin: false, roles: ["Legal"] },
};
let uploads = 0;
let failFirst = false;
let saved: Blob[] = [];

beforeEach(() => {
  uploads = 0;
  saved = [];
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  (globalThis as { matchMedia?: unknown }).matchMedia = () => ({ matches: false, addEventListener() {}, removeEventListener() {} });
  URL.createObjectURL = vi.fn((b: Blob) => {
    saved.push(b);
    return "blob:sheet";
  }) as typeof URL.createObjectURL;
  URL.revokeObjectURL = vi.fn();
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = new URL(String(input instanceof Request ? input.url : input), "http://localhost").pathname;
      const json = (b: unknown, status = 200) => new Response(JSON.stringify(b), { status, headers: { "content-type": "application/json" } });
      if (path === "/api/auth/me") return json(me);
      if (path === "/api/meta") return json({ entities: [], modules: [] });
      if (path === "/api/version") return json({ uploads: { max_bytes: 10_000_000, parts_max_bytes: 100_000_000 } });
      if (path === "/api/files" && (init?.method ?? "GET") === "POST") {
        uploads += 1;
        if (failFirst && uploads === 1) return json({ error: "the store is busy" }, 503);
        return json({ id: `f${uploads}`, url: `/files/f${uploads}`, file_name: `f${uploads}.pdf`, size: 4, sha256: "x" });
      }
      return json({ error: "not found" }, 404);
    }),
  );
  setFrameMode("app", CONTRACTS_PACKAGES);
});
afterEach(() => vi.unstubAllGlobals());

const settle = async (until: () => boolean) => {
  for (let i = 0; i < 80 && !until(); i++) await act(async () => void (await new Promise((r) => setTimeout(r, 25))));
};

async function page() {
  const host = document.createElement("div");
  document.body.append(host);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = createMemoryRouter(routes, { initialEntries: ["/signed-copies"] });
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <QueryClientProvider client={qc}>
        <RouterProvider router={router} />
      </QueryClientProvider>,
    );
  });
  await settle(() => !!host.querySelector("input[type=file]"));
  const choose = async (names: string[]) => {
    const input = host.querySelector<HTMLInputElement>("input[type=file]")!;
    const files = names.map((n) => new File(["%PDF"], n, { type: "application/pdf" }));
    Object.defineProperty(input, "files", { value: files, configurable: true });
    await act(async () => void input.dispatchEvent(new Event("change", { bubbles: true })));
  };
  const close = async () => {
    await act(async () => root.unmount());
    host.remove();
  };
  return { host, choose, close };
}

const read = (b: Blob) => new Promise<string>((ok) => {
  const r = new FileReader();
  r.onload = () => ok(String(r.result));
  r.readAsText(b);
});

describe("the signed copies page", () => {
  it("uploads each file and offers the sheet", async () => {
    failFirst = false;
    const { host, choose, close } = await page();
    expect(host.querySelector("label[for=signed-copies-files]")).not.toBeNull();
    await choose(["Globex NDA.pdf", "Initech MSA.pdf"]);
    await settle(() => (host.textContent?.match(/Uploaded/g) ?? []).length === 2);
    expect((host.textContent?.match(/Uploaded/g) ?? []).length).toBe(2);
    const download = Array.from(host.querySelectorAll("button")).find((b) => b.textContent === "Download the sheet")!;
    await act(async () => void download.click());
    const csv = (await read(saved[0])).trim().split("\n");
    expect(csv[0]).toBe(COLUMNS.join(","));
    expect(csv.slice(1)).toEqual(["Globex NDA,,,,,,,,,/files/f1", "Initech MSA,,,,,,,,,/files/f2"]);
    await close();
  });

  it("lets a failed upload be tried again", async () => {
    failFirst = true;
    const { host, choose, close } = await page();
    await choose(["Globex NDA.pdf"]);
    await settle(() => !!host.textContent?.includes("Failed"));
    expect(host.textContent).toContain("Failed");
    const retry = Array.from(host.querySelectorAll("button")).find((b) => b.textContent === "Retry")!;
    await act(async () => void retry.click());
    await settle(() => !!host.textContent?.includes("Uploaded"));
    expect(host.textContent).toContain("Uploaded");
    await close();
  });
});
