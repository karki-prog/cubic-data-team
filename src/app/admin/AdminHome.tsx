"use client";

import { Skel } from "@/components/ui/Skeleton";
import { useCallback, useEffect, useRef, useState } from "react";
import { AdminShell } from "./AdminShell";

type Template = {
  id: string;
  name: string;
  filename: string;
  size: number;
  uploaded_by: string;
  uploaded_at: string;
};

function prettySize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function prettyDate(iso: string) {
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? ""
    : d.toLocaleDateString(undefined, { day: "2-digit", month: "short", year: "numeric" });
}

export function AdminHome() {
  const [templates, setTemplates] = useState<Template[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);

  const load = useCallback(async () => {
    try {
      const res = await fetch("/api/admin/templates", { credentials: "same-origin" });
      const data = await res.json();
      if (!res.ok || !data?.ok) throw new Error(data?.error || `Failed (${res.status})`);
      setTemplates(data.templates || []);
    } catch (err) {
      setError((err as Error).message);
      setTemplates([]);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  async function upload(file: File) {
    setBusy(true);
    setError("");
    try {
      const body = new FormData();
      body.append("file", file, file.name);
      const res = await fetch("/api/admin/templates", {
        method: "POST",
        credentials: "same-origin",
        body,
      });
      const data = await res.json();
      if (!res.ok || !data?.ok) throw new Error(data?.error || `Upload failed (${res.status})`);
      setTemplates(data.templates || []);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setBusy(false);
      if (fileRef.current) fileRef.current.value = "";
    }
  }

  async function remove(t: Template) {
    if (!window.confirm(`Remove the template “${t.name}”? This deletes the file.`)) return;
    setBusy(true);
    setError("");
    try {
      const res = await fetch(`/api/admin/templates/${encodeURIComponent(t.id)}`, {
        method: "DELETE",
        credentials: "same-origin",
      });
      const data = await res.json();
      if (!res.ok || !data?.ok) throw new Error(data?.error || `Delete failed (${res.status})`);
      setTemplates(data.templates || []);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <AdminShell subtitle="Superadmin only">
      <section className="admin-panel" aria-label="Resume templates">
        <div className="tpl-head">
          <div>
            <h2 className="tpl-title">Resume templates</h2>
            <p className="tpl-sub">Pick a template to open it in Word, or add a new one.</p>
          </div>
          <label className={`tpl-add${busy ? " is-busy" : ""}`}>
            {busy ? "Working…" : "Add template"}
            <input
              ref={fileRef}
              type="file"
              accept=".docx"
              hidden
              disabled={busy}
              onChange={(e) => {
                const f = e.target.files?.[0];
                if (f) upload(f);
              }}
            />
          </label>
        </div>

        {error ? (
          <p className="tpl-error" role="alert">
            {error}
          </p>
        ) : null}

        {templates === null ? (
          <ul className="tpl-list" aria-busy="true" aria-label="Loading templates">
            {Array.from({ length: 3 }).map((_, i) => (
              <li key={i} className="tpl-row">
                <Skel w={20} h={24} r={4} />
                <Skel w="45%" h={13} />
                <Skel w={70} h={28} r={7} style={{ marginLeft: "auto" }} />
              </li>
            ))}
          </ul>
        ) : templates.length === 0 ? (
          <p className="tpl-empty">
            No templates yet. Add a .docx to get started — it becomes the starting point for a
            resume.
          </p>
        ) : (
          <ul className="tpl-list">
            {templates.map((t) => (
              <li key={t.id} className="tpl-row">
                <span className="tpl-icon" aria-hidden="true">
                  <svg width="20" height="20" viewBox="0 0 24 24" fill="none">
                    <path
                      d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8l-5-5Z"
                      stroke="currentColor"
                      strokeWidth="1.6"
                      strokeLinejoin="round"
                    />
                    <path d="M14 3v5h5" stroke="currentColor" strokeWidth="1.6" />
                  </svg>
                </span>
                <span className="tpl-meta">
                  <span className="tpl-name">{t.name}</span>
                  <span className="tpl-detail">
                    {t.filename} · {prettySize(t.size)}
                    {t.uploaded_at ? ` · added ${prettyDate(t.uploaded_at)}` : ""}
                  </span>
                </span>
                <a
                  className="tpl-open"
                  href={`/admin/resume-automation?template=${encodeURIComponent(t.id)}`}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  Open in Word
                </a>
                <button
                  type="button"
                  className="tpl-remove"
                  disabled={busy}
                  onClick={() => remove(t)}
                  aria-label={`Remove ${t.name}`}
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </AdminShell>
  );
}
