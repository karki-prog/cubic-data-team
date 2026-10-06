"use client";

import Link from "next/link";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ArrowIcon } from "@/components/ui/ArrowIcon";
import { useLiveAvailabilityPoll } from "@/lib/booking/useLiveAvailabilityPoll";
import type { HiringDayGroup, HiringJob } from "@/lib/hiring/config";
import "../book/phone-call/phone-call.css";
import "./hiring.css";

type ApiResponse = {
  ok: boolean;
  error?: string;
  groups?: HiringDayGroup[];
  jobCount?: number;
  signature?: string;
  generatedAt?: string;
};

const POLL_MS = 5000;

function cleanDayLabel(label: string) {
  return label.replace(/\s*\((Today|Yesterday)\)\s*/i, "").trim();
}

/** "Wednesday, 19 Aug 2026 (Yesterday)" -> Wed / 19 / Aug for the calendar tile. */
function dayParts(label: string) {
  const cleaned = cleanDayLabel(label);
  const m = cleaned.match(/^(\w+),\s*(\d+)\s+(\w+)/);
  if (!m) return { dow: cleaned.slice(0, 3), num: "", mon: "" };
  return { dow: m[1].slice(0, 3), num: m[2], mon: m[3].slice(0, 3) };
}

function jobLines(job: HiringJob): { title: string; subtitle: string | null } {
  return {
    title: job.displayTitle || job.jobTitle || job.title,
    subtitle: job.displaySubtitle || job.company || null,
  };
}

function metaCell(value: string | undefined, extra = "") {
  const text = String(value || "").trim();
  const empty = !text || /^(0+|null|n\/a|na|-|—)$/i.test(text);
  return {
    className: `hiring-cell${extra ? ` ${extra}` : ""}${empty ? " is-empty" : ""}`,
    text: empty ? "—" : text,
  };
}

export function HiringPostings() {
  const [data, setData] = useState<ApiResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [selectedLabel, setSelectedLabel] = useState<string | null>(null);
  const signatureRef = useRef<string | null>(null);
  const metaBurstRef = useRef(false);

  const applyPayload = useCallback((json: ApiResponse, opts?: { silent?: boolean }) => {
    if (!json?.ok) {
      if (!opts?.silent) setData(json);
      return;
    }
    const nextSig = json.signature || "";
    if (opts?.silent && signatureRef.current !== null && nextSig === signatureRef.current) {
      return;
    }
    signatureRef.current = nextSig || null;
    setData(json);
  }, []);

  const load = useCallback(
    async (opts?: { silent?: boolean }) => {
      if (!opts?.silent) setLoading(true);
      try {
        const res = await fetch("/api/job-postings", {
          cache: "no-store",
          credentials: "same-origin",
        });
        let json: ApiResponse;
        try {
          json = (await res.json()) as ApiResponse;
        } catch {
          json = {
            ok: false,
            error: res.ok ? "Invalid response" : `HTTP ${res.status}`,
          };
        }
        if (!res.ok && json?.ok !== false) {
          json = { ok: false, error: json?.error || `HTTP ${res.status}` };
        }
        applyPayload(json, opts);
        if (json?.ok && !opts?.silent && !metaBurstRef.current) {
          metaBurstRef.current = true;
          window.setTimeout(() => void load({ silent: true }), 3500);
          window.setTimeout(() => void load({ silent: true }), 12000);
        }
      } catch (err) {
        if (!opts?.silent) {
          const raw = err instanceof Error ? err.message : String(err);
          const friendly = /load failed|failed to fetch|networkerror|connection/i.test(raw)
            ? "Network error talking to the server — try refresh, or check if cubic-data.com is up"
            : raw;
          setData({
            ok: false,
            error: friendly,
          });
        }
      } finally {
        if (!opts?.silent) setLoading(false);
      }
    },
    [applyPayload]
  );

  useLiveAvailabilityPoll(load, { intervalMs: POLL_MS });

  const groups: HiringDayGroup[] = data?.groups || [];
  const jobCount = data?.jobCount || 0;

  useEffect(() => {
    if (!groups.length) {
      setSelectedLabel(null);
      return;
    }
    setSelectedLabel((prev) => {
      const prevGroup = prev ? groups.find((g) => g.label === prev) : null;
      if (prevGroup) return prev;
      const todayGroup = groups.find((g) => /\(Today\)/i.test(g.label));
      return (todayGroup || groups[0]).label;
    });
  }, [groups]);

  const selected = useMemo(
    () => groups.find((g) => g.label === selectedLabel) || null,
    [groups, selectedLabel]
  );

  return (
    <div className="pca-root hiring-root">
      <header className="site-header">
        <div className="topbar">
          <div className="topbar-inner">
            <div className="brand">
              <Link href="/" className="brand-home" aria-label="Go to Cubic Data home">
                <div className="brand-name">CUBIC</div>
                <div className="brand-sub">Technologies</div>
              </Link>
            </div>
            <div className="page-title">
              <h1>Job Hiring</h1>
            </div>
            <div className="actions">
              {groups.length > 0 ? (
                <div className="hiring-day-tabs" role="tablist" aria-label="Select posting day">
                  {groups.map((group) => {
                    const active = group.label === selectedLabel;
                    const { dow, num, mon } = dayParts(group.label);
                    const today = /\(Today\)/i.test(group.label);
                    return (
                      <button
                        key={group.label}
                        type="button"
                        role="tab"
                        aria-selected={active}
                        aria-label={`${cleanDayLabel(group.label)} — ${group.jobs.length} openings`}
                        className={`hiring-day-tab cal-tile${active ? " is-active" : ""}${
                          today ? " is-today" : ""
                        }`}
                        onClick={() => setSelectedLabel(group.label)}
                      >
                        <span className="cal-dow">{dow}</span>
                        <span className="cal-num">{num}</span>
                        <span className="cal-mon">{mon}</span>
                        <span className="hiring-day-tab-count">{group.jobs.length}</span>
                      </button>
                    );
                  })}
                </div>
              ) : null}
            </div>
          </div>
        </div>
        <div className="red-rule" />
      </header>

      <div className="pca-body">
      <main className="wrap hiring-wrap">
        {loading && !groups.length ? (
          <div className="skeleton-list" aria-busy="true" aria-label="Loading job postings">
            <section className="day-card skeleton-card" aria-hidden>
              <div className="day-head skeleton-head" />
              {Array.from({ length: 8 }).map((_, i) => (
                <div key={i} className="row">
                  <div className="skel-chip" />
                  <div className="skel-time" />
                  <div className="skel-btn" />
                </div>
              ))}
            </section>
          </div>
        ) : null}

        {data && !data.ok ? (
          <p style={{ textAlign: "center", padding: "40px 0", fontWeight: 600 }}>
            Could not load postings{data.error ? ` — ${data.error}` : ""}
          </p>
        ) : null}

        {data?.ok && !jobCount ? (
          <p style={{ textAlign: "center", padding: "40px 0", fontWeight: 600 }}>
            No postings yet
          </p>
        ) : null}

        {groups.map((group) => {
          const isSelected = group.label === (selectedLabel || selected?.label);
          return (
            <section
              key={group.label}
              className="day-card hiring-day-card"
              hidden={!isSelected}
              aria-hidden={!isSelected}
            >
              <div className="day-head">
                <div className="day-name">{cleanDayLabel(group.label)}</div>
                <div className="day-sum">{group.jobs.length} openings</div>
              </div>

              <div className="hiring-table">
                <div className="slot-head hiring-meta-head">
                  <span>#</span>
                  <span>Company</span>
                  <span>Job title</span>
                  <span>Job Site</span>
                  <span>Apply</span>
                </div>

                <div className="hiring-col-rules" aria-hidden>
                  <span />
                  <span />
                  <span />
                  <span />
                  <span />
                </div>

                <div className="hiring-rows">
                  {!group.jobs.length ? (
                    <div className="hiring-empty-row">—</div>
                  ) : null}

                  {group.jobs.map((job, index) => {
                    const { subtitle } = jobLines(job);
                    const company = metaCell(job.company || subtitle || "");
                    const role = metaCell(job.jobTitle, "hiring-job-title");
                    const jobSite = metaCell(job.jobSite);
                    return (
                      <a
                        key={`${job.url}|${job.title}`}
                        href={job.url}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="row hiring-job-row hiring-meta-row"
                      >
                        <span className="hiring-num">{index + 1}</span>
                        <span className={company.className}>{company.text}</span>
                        <span className={role.className}>
                          {role.text === "—" ? "Not listed" : role.text}
                        </span>
                        <span className={jobSite.className}>{jobSite.text}</span>
                        <span className="hiring-apply">
                          Apply
                          <ArrowIcon className="apply-arrow" size={13} />
                        </span>
                      </a>
                    );
                  })}
                </div>
              </div>
            </section>
          );
        })}
      </main>
      </div>
    </div>
  );
}
