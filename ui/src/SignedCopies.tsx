// Bring in signed contracts: upload many signed PDFs, then download a sheet with one row per file to complete and
// import from the Contracts list (each row is created Signed, with its copy, who signed and when).

import { useState } from "react";
import { isolate, saveBlob, t, uploadFile } from "@ferrum/client";
import { Alert, Badge, Button, ButtonLink, PageHead, PageStack, useShellTitle } from "@ferrum/kit";

type State = "waiting" | "uploading" | "done" | "failed";
export interface Pick {
  file: File;
  state: State;
  url?: string;
  error?: string;
}

/** The sheet's columns: the contract's fields an import fills, the copy last. */
export const COLUMNS = ["title", "counterparty_name", "counterparty_email", "contract_type", "value", "starts_on", "ends_on", "recorded_signed_by", "recorded_signed_on", "recorded_copy"];

const cell = (v: string) => (/[",\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v);

/** The sheet for the uploaded files: one row each, its title the file's name without `.pdf`, its copy the file's path. */
export function sheet(done: { name: string; url: string }[]): string {
  const rows = done.map((d) => COLUMNS.map((c) => (c === "title" ? cell(d.name.replace(/\.pdf$/i, "")) : c === "recorded_copy" ? cell(d.url) : "")).join(","));
  return [COLUMNS.join(","), ...rows].join("\n") + "\n";
}

const TONES: Record<State, "neutral" | "blue" | "green" | "red"> = { waiting: "neutral", uploading: "blue", done: "green", failed: "red" };
const WORDS: Record<State, () => string> = { waiting: () => t("Waiting"), uploading: () => t("Uploading"), done: () => t("Uploaded"), failed: () => t("Failed") };

export function SignedCopies() {
  useShellTitle(t("Signed copies"));
  const [picks, setPicks] = useState<Pick[]>([]);
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState("");
  const set = (i: number, p: Partial<Pick>) => setPicks((all) => all.map((x, j) => (j === i ? { ...x, ...p } : x)));
  const upload = async (i: number, file: File) => {
    set(i, { state: "uploading", error: undefined });
    try {
      const stored = await uploadFile(file);
      set(i, { state: "done", url: stored.url });
    } catch (e) {
      const status = (e as { status?: number }).status;
      set(i, { state: "failed", error: status === 403 ? t("You may not add files here") : e instanceof Error ? e.message : String(e) });
    }
  };
  const choose = async (files: File[]) => {
    const start = picks.length;
    setPicks((all) => [...all, ...files.map((file) => ({ file, state: "waiting" as State }))]);
    setBusy(true);
    for (const [n, file] of files.entries()) await upload(start + n, file);
    setBusy(false);
    setSaid(t("{n} files handled", { n: files.length }));
  };
  const done = picks.filter((p) => p.state === "done" && p.url);
  return (
    <PageStack>
      <PageHead title={t("Bring in signed contracts")} actions={<ButtonLink to="/e/Contract">{t("Import the completed sheet")}</ButtonLink>} />
      <p>{t("Upload the signed PDFs, download the sheet, fill in each contract's counterparty and dates, then import it from the Contracts list. Each row becomes a signed contract with its copy.")}</p>
      <div>
        <label htmlFor="signed-copies-files">{t("Signed PDFs")}</label>{" "}
        <input
          id="signed-copies-files"
          type="file"
          multiple
          accept="application/pdf"
          disabled={busy}
          onChange={(e) => {
            const files = Array.from(e.currentTarget.files ?? []);
            e.currentTarget.value = "";
            if (files.length) void choose(files);
          }}
        />
      </div>
      <div aria-live="polite" className="sr-only">
        {said}
      </div>
      {picks.some((p) => p.state === "failed") && <Alert tone="warning">{t("Some files did not upload: try them again, or download the sheet for the others.")}</Alert>}
      {picks.length > 0 && (
        <table>
          <caption>{t("Files")}</caption>
          <thead>
            <tr>
              <th scope="col">{t("File")}</th>
              <th scope="col">{t("State")}</th>
              <th scope="col" />
            </tr>
          </thead>
          <tbody>
            {picks.map((p, i) => (
              <tr key={`${p.file.name}-${i}`}>
                <td>{isolate(p.file.name)}</td>
                <td>
                  <Badge tone={TONES[p.state]}>{WORDS[p.state]()}</Badge> {p.error ? isolate(p.error) : null}
                </td>
                <td>
                  {p.state === "failed" && (
                    <Button size="sm" onClick={() => void upload(i, p.file)}>
                      {t("Retry")}
                    </Button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <div>
        <Button variant="primary" disabled={busy || done.length === 0} onClick={() => saveBlob(new Blob([sheet(done.map((d) => ({ name: d.file.name, url: d.url! })))], { type: "text/csv" }), "signed-contracts.csv")}>
          {t("Download the sheet")}
        </Button>
      </div>
    </PageStack>
  );
}
