"use client";

import Link from "next/link";
import { useCallback, useMemo, useRef, useState } from "react";
import { ArrowIcon } from "@/components/ui/ArrowIcon";
import { useLiveAvailabilityPoll } from "@/lib/booking/useLiveAvailabilityPoll";
import type { CubicInterviewsResponse, InterviewClient } from "@/lib/interviews/types";
import { Skel } from "@/components/ui/Skeleton";
import { CompanyBars, LegendKey, TrackIcon } from "./InterviewCharts";
import "../book/phone-call/phone-call.css";
import "./interviews.css";

/** The API only serves the stored copy, so a slow poll is enough to pick up a sync. */
const POLL_MS = 30000;
const TOP_COMPANIES = 10;

type Track = "all" | "data" | "java";
type SortKey = "total" | "company" | "data" | "java";

const TRACKS: { key: Track; label: string }[] = [
  { key: "all", label: "All" },
  { key: "data", label: "Data" },
  { key: "java", label: "Java" },
];

function updatedLabel(iso?: string) {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleString(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
}

/** "Origamirisk — Senior Data Engineer" → role part, else the whole label. */
function portalText(client: InterviewClient) {
  const label = client.linkLabel.trim();
  if (!label) return "Open";
  const parts = label.split(/\s+[—–-]\s+/);
  return parts.length > 1 ? parts.slice(1).join(" — ") : label;
}

function inTrack(c: InterviewClient, track: Track) {
  return track === "all" || (track === "data" ? c.data > 0 : c.java > 0);
}

/** Interviews a company counts for under the selected track. */
function trackCount(c: InterviewClient, track: Track) {
  return track === "all" ? c.total : track === "data" ? c.data : c.java;
}

/** Bar segments for a company: the full split under All, only the selected track otherwise. */
function segments(c: InterviewClient, track: Track) {
  if (track === "data") return { data: c.data, java: 0, other: 0 };
  if (track === "java") return { data: 0, java: c.java, other: 0 };
  return { data: c.data, java: c.java, other: Math.max(0, c.total - c.data - c.java) };
}

export function CubicInterviews() {
  const [data, setData] = useState<CubicInterviewsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [highlight, setHighlight] = useState<string | null>(null);
  const [track, setTrack] = useState<Track>("all");
  const [sort, setSort] = useState<SortKey>("total");
  const signatureRef = useRef<string | null>(null);
  const tableRef = useRef<HTMLElement>(null);
  const highlightTimer = useRef<number | undefined>(undefined);

  const load = useCallback(async (opts?: { silent?: boolean }) => {
    if (!opts?.silent) setLoading(true);
    try {
      const res = await fetch("/api/cubic-interviews", { cache: "no-store", credentials: "same-origin" });
      let json: CubicInterviewsResponse;
      try {
        json = (await res.json()) as CubicInterviewsResponse;
      } catch {
        json = { ok: false, error: res.ok ? "Invalid response" : `HTTP ${res.status}` };
      }
      if (!res.ok && json?.ok !== false) {
        json = { ok: false, error: json?.error || `HTTP ${res.status}` };
      }
      if (!json.ok) {
        if (!opts?.silent) setData(json);
        return;
      }
      const sig = json.signature || "";
      if (opts?.silent && signatureRef.current !== null && sig === signatureRef.current) return;
      signatureRef.current = sig || null;
      setData(json);
    } catch (err) {
      if (!opts?.silent) {
        const raw = err instanceof Error ? err.message : String(err);
        setData({
          ok: false,
          error: /load failed|failed to fetch|networkerror|connection/i.test(raw)
            ? "Network error talking to the server — try refresh"
            : raw,
        });
      }
    } finally {
      if (!opts?.silent) setLoading(false);
    }
  }, []);

  useLiveAvailabilityPoll(load, { intervalMs: POLL_MS });

  const clients = useMemo(() => data?.clients ?? [], [data]);

  /** Interviews + companies per track, for the header tiles. */
  const trackTotals = useMemo(() => {
    const out = {} as Record<Track, { interviews: number; companies: number }>;
    for (const t of TRACKS) {
      const list = clients.filter((c) => inTrack(c, t.key));
      out[t.key] = {
        companies: list.length,
        interviews: list.reduce((n, c) => n + trackCount(c, t.key), 0),
      };
    }
    return out;
  }, [clients]);

  const scoped = useMemo(() => clients.filter((c) => inTrack(c, track)), [clients, track]);

  const summary = useMemo(() => {
    let total = 0;
    let dataN = 0;
    let javaN = 0;
    for (const c of scoped) {
      // A track filter counts only that track, so the headline matches its header tile.
      total += trackCount(c, track);
      dataN += c.data;
      javaN += c.java;
    }
    return { total, data: dataN, java: javaN, companies: scoped.length };
  }, [scoped, track]);

  const topCompanies = useMemo(
    () =>
      [...scoped]
        .sort((a, b) => trackCount(b, track) - trackCount(a, track) || a.company.localeCompare(b.company))
        .slice(0, TOP_COMPANIES)
        .map((c) => ({ company: c.company, total: trackCount(c, track), ...segments(c, track) })),
    [scoped, track]
  );

  const rows = useMemo(() => {
    const byName = (a: InterviewClient, b: InterviewClient) =>
      a.company.localeCompare(b.company, undefined, { sensitivity: "base" });
    const value = (c: InterviewClient) => (sort === "total" ? trackCount(c, track) : sort === "data" ? c.data : c.java);
    return [...scoped].sort((a, b) => {
      if (sort === "company") return byName(a, b);
      const diff = value(b) - value(a);
      return diff !== 0 ? diff : trackCount(b, track) - trackCount(a, track) || byName(a, b);
    });
  }, [scoped, sort, track]);

  const maxTotal = Math.max(1, ...scoped.map((c) => trackCount(c, track)));
  const split = summary.data + summary.java;

  /** Top-companies click: scroll the table to that row and flash it. */
  const focusCompany = (company: string) => {
    setHighlight(company);
    const row = tableRef.current?.querySelector<HTMLElement>(`[data-company="${CSS.escape(company)}"]`);
    row?.scrollIntoView({ behavior: "smooth", block: "center" });
    window.clearTimeout(highlightTimer.current);
    highlightTimer.current = window.setTimeout(() => setHighlight(null), 2400);
  };

  const sortHeader = (key: SortKey, label: string) => (
    <button
      type="button"
      className={`ci-sort${sort === key ? " is-active" : ""}`}
      onClick={() => setSort(key)}
      aria-pressed={sort === key}
    >
      {label}
      <span className="ci-sort-caret" aria-hidden>
        {sort === key ? (key === "company" ? "▲" : "▼") : "↕"}
      </span>
    </button>
  );

  return (
    <div className="pca-root ci-root">
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
              <h1>Cubic Interviews</h1>
              <div className="page-subtitle"><span className="ci-sub-lead">Interviews received · </span>Past 15 days &amp; next 5 days</div>
            </div>
            <div className="actions">
              {data?.ok ? (
                <label className="ci-track-pick">
                  <span className="ci-sr">Track</span>
                  <select value={track} onChange={(e) => setTrack(e.target.value as Track)}>
                    {TRACKS.map((t) => (
                      <option key={t.key} value={t.key}>
                        {t.label} · {trackTotals[t.key].interviews}
                      </option>
                    ))}
                  </select>
                  <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden>
                    <path d="M2 3.5l3 3 3-3" stroke="currentColor" strokeWidth="1.6" fill="none" />
                  </svg>
                </label>
              ) : null}
            </div>
          </div>
        </div>
        <div className="red-rule" />
      </header>

      <div className="pca-body">
        <main className={`wrap ci-wrap${loading && data ? " is-refreshing" : ""}`}>
          {loading && !data ? (
            <div className="ci-loading" aria-busy="true" aria-label="Loading interviews">
              <div className="ci-top-grid">
                <div className="cubic-skel-card">
                  <div className="ci-skel-band"><Skel w={150} h={13} style={{ margin: "0 auto" }} /></div>
                  <div style={{ padding: "22px 24px", display: "grid", gap: 12 }}>
                    <Skel w={130} h={56} r={8} />
                    <Skel w={150} h={13} />
                    <Skel h={12} r={4} style={{ marginTop: 10 }} />
                    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
                      <Skel h={58} r={8} />
                      <Skel h={58} r={8} />
                    </div>
                  </div>
                </div>
                <div className="cubic-skel-card">
                  <div className="ci-skel-band"><Skel w={150} h={13} style={{ margin: "0 auto" }} /></div>
                  <div style={{ padding: "14px 22px", display: "grid", gap: 14 }}>
                    {[96, 80, 64, 52, 52, 52, 40, 40, 40, 40].map((w, i) => (
                      <div key={i} style={{ display: "grid", gridTemplateColumns: "20px 26% 1fr", gap: 12, alignItems: "center" }}>
                        <Skel w={12} h={10} />
                        <Skel w="80%" h={11} />
                        <Skel w={`${w}%`} h={14} r={4} />
                      </div>
                    ))}
                  </div>
                </div>
              </div>
              <div className="cubic-skel-card">
                <div className="ci-skel-band"><Skel w={150} h={13} style={{ margin: "0 auto" }} /></div>
                {Array.from({ length: 8 }).map((_, i) => (
                  <div key={i} style={{ display: "grid", gridTemplateColumns: "40px 1.3fr 0.9fr 70px 70px 1fr 1.3fr 1.2fr", gap: 16, padding: "15px 18px", borderTop: "1px solid #eef0f3" }}>
                    <Skel w={16} h={10} />
                    <Skel w="70%" h={12} />
                    <Skel w="60%" h={12} />
                    <Skel w={20} h={12} />
                    <Skel w={20} h={12} />
                    <Skel w="60%" h={11} />
                    <Skel w="80%" h={11} />
                    <Skel w="50%" h={11} />
                  </div>
                ))}
              </div>
            </div>
          ) : null}

          {data && !data.ok ? (
            <p className="ci-message">Could not load interviews{data.error ? ` — ${data.error}` : ""}</p>
          ) : null}

          {data?.ok ? (
            <>
              <div className="ci-top-grid">
                {/* The headline: how many interviews, from how many companies */}
                <section className="day-card ci-panel ci-hero" aria-label="Interviews received">
                  <div className="day-head">
                    <div className="day-name">
                      {track === "all" ? "Interviews received" : `${track === "data" ? "Data" : "Java"} interviews received`}
                    </div>
                  </div>
                  <div className="ci-hero-body">
                    <div className="ci-hero-num">{summary.total.toLocaleString()}</div>
                    <div className="ci-hero-from">
                      from <b>{summary.companies}</b> {summary.companies === 1 ? "company" : "companies"}
                    </div>

                    {track === "all" ? (
                    <>
                    <div className="ci-split" aria-label={`${summary.data} data, ${summary.java} java`}>
                      {summary.data ? <span className="ci-seg ci-seg-data" style={{ flexGrow: summary.data }} /> : null}
                      {summary.java ? <span className="ci-seg ci-seg-java" style={{ flexGrow: summary.java }} /> : null}
                      {!split ? <span className="ci-seg ci-seg-other" style={{ flexGrow: 1 }} /> : null}
                    </div>
                    <div className="ci-track-stats">
                      <div className="ci-track-stat is-data">
                        <span className="ci-track-stat-label">
                          <TrackIcon track="data" size={18} />
                          Data
                        </span>
                        <b>{summary.data}</b>
                      </div>
                      <div className="ci-track-stat is-java">
                        <span className="ci-track-stat-label">
                          <TrackIcon track="java" size={18} />
                          Java
                        </span>
                        <b>{summary.java}</b>
                      </div>
                    </div>
                    </>
                    ) : (
                    <div className="ci-track-note">
                      <LegendKey tone={track} label={track === "data" ? "Data track only" : "Java track only"} />
                    </div>
                    )}

                    <dl className="ci-hero-meta">
                      {data.generatedAt ? (
                        <div>
                          <dt>Synced</dt>
                          <dd>{updatedLabel(data.generatedAt)}</dd>
                        </div>
                      ) : null}
                    </dl>
                  </div>
                </section>

                <CompanyBars rows={topCompanies} onSelect={focusCompany} />
              </div>

              {/* Every company, in the same table style as Job Hiring */}
              <section className="day-card ci-panel ci-list" ref={tableRef}>
                <div className="day-head ci-list-head">
                  <div className="day-name">All companies</div>
                  <div className="day-sum">
                    {rows.length} {rows.length === 1 ? "company" : "companies"} · {summary.total} interviews
                  </div>
                </div>

                <div className="ci-table-scroll">
                  <div className="ci-table" role="table" aria-label="Interviews received by company">
                    <div className="ci-row ci-head" role="row">
                      <span role="columnheader">#</span>
                      <span role="columnheader">{sortHeader("company", "Company")}</span>
                      <span role="columnheader">{sortHeader("total", "Interviews")}</span>
                      <span role="columnheader" className="ci-center">{sortHeader("data", "Data")}</span>
                      <span role="columnheader" className="ci-center">{sortHeader("java", "Java")}</span>
                      <span role="columnheader">Visa</span>
                      <span role="columnheader">Interview dates</span>
                      <span role="columnheader">Job portal</span>
                    </div>

                    {!rows.length ? (
                      <div className="ci-empty">
                        {clients.length ? "No companies in this track." : "No interviews yet."}
                      </div>
                    ) : null}

                    {rows.map((c, i) => {
                      const count = trackCount(c, track);
                      const seg = segments(c, track);
                      return (
                        <div
                          key={c.company}
                          className={`ci-row${highlight === c.company ? " is-highlight" : ""}`}
                          role="row"
                          data-company={c.company}
                        >
                          <span className="ci-idx" role="cell">{i + 1}</span>
                          <span className="ci-company" role="cell">
                            {c.company}
                            {c.comment ? <small>{c.comment}</small> : null}
                          </span>
                          <span className="ci-count-cell" role="cell">
                            <b className="ci-count">{count}</b>
                            <span className="ci-mini" style={{ width: `${(count / maxTotal) * 100}%` }}>
                              {seg.data ? <span className="ci-seg ci-seg-data" style={{ flexGrow: seg.data }} /> : null}
                              {seg.java ? <span className="ci-seg ci-seg-java" style={{ flexGrow: seg.java }} /> : null}
                              {seg.other ? <span className="ci-seg ci-seg-other" style={{ flexGrow: seg.other }} /> : null}
                            </span>
                          </span>
                          <span className={`ci-center${c.data ? "" : " is-empty"}`} role="cell">{c.data || "—"}</span>
                          <span className={`ci-center${c.java ? "" : " is-empty"}`} role="cell">{c.java || "—"}</span>
                          <span className={`ci-visa-text${c.visas.length ? "" : " is-empty"}`} role="cell">
                            {c.visas.length
                              ? c.visas.map((v) => (v.count > 1 ? `${v.label} (${v.count})` : v.label)).join(", ")
                              : "—"}
                          </span>
                          <span className="ci-dates" role="cell">
                            {c.interviewDates.length ? c.interviewDates.join(", ") : <span className="is-empty">—</span>}
                          </span>
                          <span role="cell">
                            {c.url ? (
                              <a className="ci-portal" href={c.url} target="_blank" rel="noopener noreferrer" title={c.linkLabel || c.url}>
                                <span>{portalText(c)}</span>
                                <ArrowIcon className="ci-portal-arrow" size={12} />
                              </a>
                            ) : (
                              <span className="is-empty">—</span>
                            )}
                          </span>
                        </div>
                      );
                    })}
                  </div>
                </div>
              </section>
            </>
          ) : null}
        </main>
      </div>
    </div>
  );
}
