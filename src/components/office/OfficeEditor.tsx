"use client";

import { useEffect, useRef, useState } from "react";

declare global {
  interface Window {
    DocsAPI?: {
      DocEditor: new (el: string, config: unknown) => { destroyEditor?: () => void };
    };
  }
}

type Status = "idle" | "loading" | "ready" | "error";

/** Loads the Document Server's api.js once, no matter how many editors mount. */
function loadDocsApi(docserverUrl: string): Promise<void> {
  if (window.DocsAPI) return Promise.resolve();
  const src = `${docserverUrl.replace(/\/+$/, "")}/web-apps/apps/api/documents/api.js`;
  const existing = document.querySelector<HTMLScriptElement>(`script[data-office-api="1"]`);
  if (existing) {
    return new Promise((resolve, reject) => {
      existing.addEventListener("load", () => resolve());
      existing.addEventListener("error", () => reject(new Error("api.js failed to load")));
    });
  }
  return new Promise((resolve, reject) => {
    const s = document.createElement("script");
    s.src = src;
    s.async = true;
    s.dataset.officeApi = "1";
    s.onload = () => resolve();
    s.onerror = () =>
      reject(new Error(`Could not load the editor from ${docserverUrl}. Is it reachable?`));
    document.head.appendChild(s);
  });
}

/**
 * Embeds the self-hosted ONLYOFFICE editor.
 *
 * The signed config comes from our backend — the browser never sees the JWT
 * secret. Note `url` must be fetchable *by the Document Server*, which is a
 * different network position than the browser: a localhost URL will load the
 * editor chrome and then fail to open the file.
 */
export function OfficeEditor({
  url,
  template,
  title,
  mode = "edit",
}: {
  /** Absolute URL the Document Server can fetch. Ignored when `template` is set. */
  url?: string;
  /** Template id — the server resolves it to a signed download URL. */
  template?: string;
  /** Optional. Omit for templates: the server knows the real filename, and a
   *  title without an extension corrupts the document's fileType. */
  title?: string;
  mode?: "edit" | "view";
}) {
  const holderId = useRef(`office-editor-${Math.random().toString(36).slice(2)}`);
  const editorRef = useRef<{ destroyEditor?: () => void } | null>(null);
  const [status, setStatus] = useState<Status>("idle");
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    setStatus("loading");
    setError("");

    (async () => {
      const params = new URLSearchParams({ mode });
      if (title) params.set("title", title);
      if (template) params.set("template", template);
      else if (url) params.set("url", url);
      const res = await fetch(`/api/admin/office/editor-config?${params}`, {
        credentials: "same-origin",
      });
      const data = await res.json();
      if (cancelled) return;
      if (!res.ok || !data?.ok) {
        throw new Error(data?.error || `Editor config failed (${res.status})`);
      }
      await loadDocsApi(data.docserverUrl);
      if (cancelled) return;
      if (!window.DocsAPI) throw new Error("Editor API did not initialise.");
      editorRef.current = new window.DocsAPI.DocEditor(holderId.current, data.config);
      setStatus("ready");
    })().catch((err: Error) => {
      if (cancelled) return;
      setError(err.message);
      setStatus("error");
    });

    return () => {
      cancelled = true;
      try {
        editorRef.current?.destroyEditor?.();
      } catch {
        /* editor already gone */
      }
      editorRef.current = null;
    };
  }, [url, template, title, mode]);

  return (
    <div className="office-wrap">
      {status === "error" ? (
        <div className="office-error" role="alert">
          <strong>Editor could not open.</strong>
          <p>{error}</p>
        </div>
      ) : null}
      {status === "loading" ? <p className="office-loading">Opening editor…</p> : null}
      <div id={holderId.current} className="office-frame" data-status={status} />
    </div>
  );
}
