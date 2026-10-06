"use client";

import Link from "next/link";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { SiteFooter } from "@/components/ui/SiteFooter";
import { ApplyTallyPad, AppliesAreaChart, ChartPeriodSelect, OutcomePieChart } from "./TrackerCharts";
import type { PeriodOption } from "./TrackerCharts";
import { IconChevron, IconDoc, IconFlag, IconHistory, IconSearch } from "./TrackerIcons";
import "../book/phone-call/phone-call.css";
import "./tracker.css";

type ApplyDay = {
  day: number;
  label: string;
  count: number;
  isToday?: boolean;
  inThisWeek?: boolean;
};

/** One company, merging booked interviews with ones logged before booking. */
type CompanyRow = {
  company: string;
  type?: string;
  roundLabel: string;
  roundDepth: number;
  interviews: number;
  phoneCalls?: number;
  booked: number;
  logged: number;
  lastDate: string;
  lastDateKey?: string;
  lastTime?: string;
  status: string;
  resumeUrl?: string;
  jdUrl?: string;
};

type CandidateOption = {
  name: string;
  email: string;
  /** Current_Market roster columns — admin view only. */
  mktStartDate?: string;
  eadEndDate?: string;
  visaStatus?: string;
  marketStage?: string;
  marketingLocation?: string;
  nepalPoc?: string;
  currentLocation?: string;
  phone?: string;
};

type TrackerResponse = {
  ok: boolean;
  admin?: boolean;
  matched?: boolean;
  mocked?: boolean;
  message?: string;
  error?: string;
  profile?: { name?: string; email?: string };
  candidates?: CandidateOption[];
  applies?: {
    monthTab?: string;
    total?: number;
    thisWeek?: number;
    byDay?: ApplyDay[];
    matched?: boolean;
    lifetime?: number;
    today?: number;
    byWeek?: ApplyDay[];
    byMonth?: { label: string; tab?: string; value: number; isCurrent?: boolean }[];
    months?: PeriodOption[];
    weeks?: PeriodOption[];
  };
  phoneCalls?: {
    total?: number;
    thisMonth?: number;
    thisWeek?: number;
    byDay?: ApplyDay[];
    byWeek?: ApplyDay[];
    byMonth?: { label: string; tab?: string; value: number; isCurrent?: boolean }[];
    months?: PeriodOption[];
    weeks?: PeriodOption[];
    recent?: { company: string; date?: string; dateKey?: string; time?: string; status?: string }[];
    retentionDays?: number;
  };
  outcomes?: {
    pending?: number;
    rejected?: number;
    offered?: number;
  };
  interviews?: {
    total?: number;
    companyCount?: number;
    booked?: number;
    logged?: number;
    phoneCalls?: number;
    historyMonths?: number;
    avgRoundsReached?: number;
    maxRoundDepth?: number;
    deepestRound?: string;
  };
  companies?: CompanyRow[];
};

/** Roster cell: one line, ellipsis, full value on hover. */
function Cell({ className, value }: { className: string; value?: string }) {
  const text = String(value || "").trim();
  if (!text) return <td className={`${className} tracker-cell-empty`}>—</td>;
  return (
    <td className={className} title={text}>
      {text}
    </td>
  );
}

function statusLabel(status: string) {
  const s = String(status || "").trim().toLowerCase();
  if (/(cancel|reject|declin|no.?show|drop|fail|withdraw|miss)/.test(s)) return "Rejected";
  if (/(accept|confirm|complete|done|pass|select|offer|hired|attended)/.test(s)) return "Accepted";
  return "Pending";
}

function hasDoc(url?: string) {
  return Boolean(openableUrl(url));
}

function openableUrl(url?: string) {
  const raw = String(url || "").trim();
  if (!raw || raw === "—") return "";
  if (/^https?:\/\//i.test(raw)) return raw;
  if (/^(www\.|drive\.google\.com|docs\.google\.com)/i.test(raw)) return `https://${raw}`;
  return "";
}

/** No resume/JD (or blank status) shows the same centered dash as the doc cells. */
function companyStatus(c: CompanyRow) {
  if (!hasDoc(c.resumeUrl) && !hasDoc(c.jdUrl)) return "";
  return String(c.status || "").trim();
}

/** One table format: MM/DD/YYYY, from the ISO key when we have it. */
function formatInterviewDate(raw?: string, key?: string) {
  const pad = (n: number) => String(n).padStart(2, "0");
  const fromParts = (year: number, month: number, day: number) => {
    if (!year || !month || !day) return "";
    return `${pad(month)}/${pad(day)}/${year}`;
  };
  const iso = String(key || "").trim();
  const isoMatch = iso.match(/^(\d{4})-(\d{2})-(\d{2})/);
  if (isoMatch) {
    return fromParts(Number(isoMatch[1]), Number(isoMatch[2]), Number(isoMatch[3]));
  }
  const text = String(raw || "").trim();
  if (!text) return "—";
  const ymd = text.match(/^(\d{4})-(\d{1,2})-(\d{1,2})/);
  if (ymd) return fromParts(Number(ymd[1]), Number(ymd[2]), Number(ymd[3]));
  const mdy = text.match(/^(\d{1,2})[/-](\d{1,2})[/-](\d{2,4})/);
  if (mdy) {
    let year = Number(mdy[3]);
    if (year < 100) year += 2000;
    return fromParts(year, Number(mdy[1]), Number(mdy[2]));
  }
  return text;
}

function docLink(url: string | undefined, label: string, kind: "resume" | "jd") {
  const href = openableUrl(url);
  if (!href) return <span className="tracker-doc-empty">—</span>;
  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      className={`tracker-doc-btn tracker-doc-${kind}`}
    >
      <IconDoc />
      {label}
    </a>
  );
}

function pickPeriod(options: PeriodOption[], key: string) {
  return options.find((o) => o.key === key) || options.find((o) => o.isCurrent) || options[0];
}

function periodBars(
  period: PeriodOption | undefined,
  unit: string,
  unitPlural: string,
  weekLabels = false
) {
  return (period?.byDay || []).map((d) => ({
    label: weekLabels ? d.label.slice(0, 3) : String(d.day),
    value: d.count,
    active: Boolean(d.isToday),
    title: `${d.label} ${d.day}: ${d.count} ${d.count === 1 ? unit : unitPlural}`,
  }));
}

function TrackerSkeleton() {
  return (
    <div className="tracker-skeleton" aria-busy="true" aria-label="Loading tracker">
      <div className="tracker-skel tracker-skel-head" />
      <div className="tracker-skel-tiles">
        {Array.from({ length: 6 }).map((_, i) => (
          <div key={i} className="tracker-skel tracker-skel-tile" />
        ))}
      </div>
      <div className="tracker-skel-charts">
        {Array.from({ length: 4 }).map((_, i) => (
          <div key={i} className="tracker-skel tracker-skel-chart" />
        ))}
      </div>
    </div>
  );
}

const TRACKER_CACHE_PREFIX = "cubic-tracker:v2:";

function trackerCacheKey(email?: string): string {
  return TRACKER_CACHE_PREFIX + (email ? email.trim().toLowerCase() : "__list__");
}

function readTrackerCache(email?: string): TrackerResponse | null {
  try {
    const raw = sessionStorage.getItem(trackerCacheKey(email));
    return raw ? (JSON.parse(raw) as TrackerResponse) : null;
  } catch {
    return null;
  }
}

function writeTrackerCache(email: string | undefined, json: TrackerResponse): void {
  try {
    if (json && json.ok !== false) {
      sessionStorage.setItem(trackerCacheKey(email), JSON.stringify(json));
    }
  } catch {
    // storage full / unavailable — the tracker still works, just uncached.
  }
}

export function ApplicationTracker() {
  const [data, setData] = useState<TrackerResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [selectedEmail, setSelectedEmail] = useState("");
  const [pocFilter, setPocFilter] = useState<"all" | "Sajit" | "Prasanna" | "Saksham">("all");
  const [companyFilter, setCompanyFilter] = useState("");
  const [candidatesCache, setCandidatesCache] = useState<CandidateOption[]>([]);
  const [todayInput, setTodayInput] = useState("");
  const [savingApplies, setSavingApplies] = useState(false);
  const [applySaveMsg, setApplySaveMsg] = useState("");
  const [applyMonthKey, setApplyMonthKey] = useState("");
  const [applyWeekKey, setApplyWeekKey] = useState("");
  // What the UI is currently showing: "" = candidate list, else the email.
  // Set synchronously on navigation so a slow/background response for a view
  // the user already left can be discarded instead of clobbering the screen.
  const viewRef = useRef<string>("");

  const load = useCallback(async (email?: string, background = false) => {
    const wantView = (email || "").trim().toLowerCase();
    if (!background) setLoading(true);
    try {
      const qs = email ? `?email=${encodeURIComponent(email)}` : "";
      const res = await fetch(`/api/application-tracker${qs}`, {
        cache: "no-store",
        credentials: "same-origin",
      });
      let json: TrackerResponse;
      try {
        json = (await res.json()) as TrackerResponse;
      } catch {
        json = { ok: false, error: res.ok ? "Invalid response" : `HTTP ${res.status}` };
      }
      if (!res.ok && json.ok !== false) {
        json = { ok: false, error: json.error || `HTTP ${res.status}` };
      }
      // Stale response — the user has navigated elsewhere since this fired.
      if (wantView !== viewRef.current) {
        if (json.ok !== false) writeTrackerCache(email, json);
        return;
      }
      // A background refresh must not wipe good data with a transient error.
      if (background && json.ok === false) return;
      setData(json);
      writeTrackerCache(email, json);
      if (json.candidates?.length) {
        setCandidatesCache(json.candidates);
        writeTrackerCache("__candidates__", { ok: true, candidates: json.candidates });
      }
    } catch (err) {
      if (background) return;
      const raw = err instanceof Error ? err.message : String(err);
      setData({
        ok: false,
        error: /load failed|failed to fetch|networkerror/i.test(raw)
          ? "Could not reach the server. Try refresh."
          : raw,
      });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    // Show the last-seen data instantly, then revalidate in the background so
    // the tracker never blocks on the (slow, Sheets-bound) API.
    const cachedList = readTrackerCache();
    const cachedCandidates = readTrackerCache("__candidates__");
    if (cachedCandidates?.candidates?.length) {
      setCandidatesCache(cachedCandidates.candidates);
    }
    viewRef.current = "";
    if (cachedList) {
      setData(cachedList);
      setLoading(false);
    }
    void load(undefined, Boolean(cachedList));
  }, [load]);

  useEffect(() => {
    setApplySaveMsg("");
    setApplyMonthKey("");
    setApplyWeekKey("");
  }, [data?.profile?.email]);

  useEffect(() => {
    if (data?.applies?.today == null || data.matched === false) {
      setTodayInput("");
      return;
    }
    const today = Number(data.applies.today);
    setTodayInput(today > 0 ? String(today) : "");
  }, [data?.applies?.today, data?.matched, data?.profile?.email]);

  const candidates = useMemo(() => {
    const source = candidatesCache.length ? candidatesCache : data?.candidates || [];
    // Admin roster only lists candidates whose tracker can actually be opened —
    // i.e. that have a personal email on Current_Market.
    const raw = source.filter((c) => String(c.email || "").trim());
    const ranked = [...raw].sort((a, b) => {
      const fill = (c: CandidateOption) =>
        Number(Boolean(c.mktStartDate || c.marketStage || c.phone));
      return fill(b) - fill(a);
    });
    const seenEmail = new Set<string>();
    const seenName = new Set<string>();
    const unique: CandidateOption[] = [];
    for (const c of ranked) {
      const email = String(c.email || "")
        .trim()
        .toLowerCase();
      const name = String(c.name || "")
        .trim()
        .toLowerCase()
        .replace(/\s+/g, " ");
      if (email && seenEmail.has(email)) continue;
      if (name && seenName.has(name)) continue;
      if (email) seenEmail.add(email);
      if (name) seenName.add(name);
      unique.push(c);
    }
    unique.sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
    return unique;
  }, [candidatesCache, data?.candidates]);
  const filteredCandidates = useMemo(() => {
    if (pocFilter === "all") return candidates;
    return candidates.filter((c) => {
      const poc = String(c.nepalPoc || "").trim().toLowerCase();
      if (pocFilter === "Prasanna") return poc.startsWith("pras");
      if (pocFilter === "Sajit") return poc.startsWith("sajit");
      return poc.startsWith("saksham") || poc.startsWith("shaksham");
    });
  }, [candidates, pocFilter]);

  const isAdmin = Boolean(data?.admin);
  const showList = Boolean(data?.ok && isAdmin && !selectedEmail && !loading);
  const showDetail = Boolean(
    data?.ok &&
      !loading &&
      ((!isAdmin && data.matched !== false) || (isAdmin && selectedEmail && data.matched))
  );
  const showDetailEmpty = Boolean(
    data?.ok && !loading && isAdmin && selectedEmail && !data.matched
  );

  const applies = data?.applies;
  const monthTabLabel = (applies?.monthTab || "").replace(/_/g, " ");
  const interviews = data?.interviews;
  const phoneCalls = data?.phoneCalls;
  const allCompanies = useMemo(() => data?.companies || [], [data?.companies]);
  const companies = useMemo(() => {
    const q = companyFilter.trim().toLowerCase();
    if (!q) return allCompanies;
    return allCompanies.filter(
      (c) =>
        c.company.toLowerCase().includes(q) ||
        c.roundLabel.toLowerCase().includes(q) ||
        String(c.type || "").toLowerCase().includes(q) ||
        String(c.status || "").toLowerCase().includes(q)
    );
  }, [allCompanies, companyFilter]);
  const applyMonths = applies?.months || [];
  const applyWeeks = applies?.weeks || [];
  const selectedApplyMonth = pickPeriod(applyMonths, applyMonthKey);
  const selectedApplyWeek = pickPeriod(applyWeeks, applyWeekKey);
  const monthBars = periodBars(selectedApplyMonth, "apply", "applies");
  const weekBars = periodBars(selectedApplyWeek, "apply", "applies", true);
  const lifetimeBars = useMemo(
    () =>
      (applies?.byMonth || []).map((d) => ({
        label: d.label,
        value: d.value,
        active: Boolean(d.isCurrent),
        title: `${(d.tab || d.label || "").replace(/_/g, " ")}: ${d.value} ${
          d.value === 1 ? "apply" : "applies"
        }`,
      })),
    [applies?.byMonth]
  );
  const phoneBars = useMemo(
    () =>
      (phoneCalls?.byMonth || []).map((d) => ({
        label: d.label,
        value: d.value,
        active: Boolean(d.isCurrent),
        title: `${d.label}: ${d.value} ${d.value === 1 ? "phone call" : "phone calls"}`,
      })),
    [phoneCalls?.byMonth]
  );
  const phoneHasData = phoneBars.some((p) => p.value > 0);

  const openCandidate = (email: string) => {
    viewRef.current = email.trim().toLowerCase();
    setCompanyFilter("");
    setSelectedEmail(email);
    const cached = readTrackerCache(email);
    setData(cached ?? null);
    setLoading(!cached);
    void load(email, Boolean(cached));
  };

  const backToList = () => {
    viewRef.current = "";
    setSelectedEmail("");
    const cached = readTrackerCache();
    if (cached) setData(cached);
    void load(undefined, Boolean(cached));
  };

  const showBack = Boolean(isAdmin && selectedEmail && !loading);
  const profileName = data?.profile?.name || data?.profile?.email || "Candidate";

  const saveTodayApplies = async () => {
    const raw = String(todayInput).trim();
    const n = raw === "" ? 0 : Number.parseInt(raw, 10);
    if (!Number.isInteger(n) || n < 0 || n > 999) {
      setApplySaveMsg("Enter a whole number between 0 and 999.");
      return;
    }
    setSavingApplies(true);
    setApplySaveMsg("");
    try {
      const res = await fetch("/api/application-tracker/applies", {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          count: n,
          ...(isAdmin && selectedEmail ? { email: selectedEmail } : {}),
        }),
      });
      const json = (await res.json().catch(() => ({}))) as {
        ok?: boolean;
        error?: string;
      };
      if (!res.ok || json.ok === false) {
        setApplySaveMsg(json.error || "Could not save today's applies.");
        return;
      }
      setTodayInput(n > 0 ? String(n) : "");
      setApplySaveMsg("Saved to the tracking sheet.");
      await load(selectedEmail || undefined, true);
    } catch (err) {
      const rawErr = err instanceof Error ? err.message : String(err);
      setApplySaveMsg(rawErr || "Could not save today's applies.");
    } finally {
      setSavingApplies(false);
    }
  };

  return (
    <div className="pca-root tracker-root">
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
              <h1>Application Tracker</h1>
            </div>
            <div className="actions">
              {showBack ? (
                <button type="button" className="tracker-btn tracker-back" onClick={backToList}>
                  All candidates
                </button>
              ) : null}
            </div>
          </div>
        </div>
        <div className="red-rule" />
      </header>

      <div className="pca-body">
        <main className={`tracker-main${showList ? " is-wide" : ""}`}>
          {loading ? <TrackerSkeleton /> : null}

          {!loading && data && !data.ok ? (
            <section className="tracker-emptyview" aria-live="polite">
              <div className="tracker-emptyview-card">
                <span className="tracker-emptyview-icon is-error">
                  <IconFlag size={20} />
                </span>
                <h2 className="tracker-emptyview-title">Couldn&apos;t load the tracker</h2>
                <p className="tracker-emptyview-text">
                  {data.error || "Something went wrong. Try refreshing the page."}
                </p>
                {isAdmin && selectedEmail ? (
                  <button
                    type="button"
                    className="tracker-btn tracker-back"
                    onClick={backToList}
                  >
                    Back to candidates
                  </button>
                ) : null}
              </div>
            </section>
          ) : null}

          {!loading && showList ? (
            <section className="tracker-candidate-list" aria-label="Candidates">
              <div className="tracker-list-card">
                <div className="tracker-list-headbar">
                  <div className="tracker-list-head-copy">
                    <p className="tracker-kicker">Admin view</p>
                    <div className="tracker-list-title-row">
                      <h2 className="tracker-list-title">Candidates</h2>
                      <span className="tracker-list-count" aria-label="Candidate count">
                        {filteredCandidates.length}
                      </span>
                    </div>
                    <p className="tracker-list-sub">
                      Current_Market roster · View opens a candidate&apos;s tracker as they see it
                    </p>
                  </div>
                  <div className="tracker-list-tools">
                    <label className="tracker-poc-filter">
                      <span className="tracker-sr-only">Filter candidates by POC</span>
                      <select
                        className="tracker-poc-select"
                        value={pocFilter}
                        onChange={(e) =>
                          setPocFilter(e.target.value as typeof pocFilter)
                        }
                        aria-label="Filter candidates by POC"
                      >
                        <option value="all">All candidates</option>
                        <option value="Sajit">Sajit</option>
                        <option value="Prasanna">Prasanna</option>
                        <option value="Saksham">Saksham</option>
                      </select>
                    </label>
                  </div>
                </div>

                <div
                  className="tracker-list-scroll"
                  role="region"
                  aria-label="Scrollable candidate table"
                  tabIndex={0}
                >
                  <table className="tracker-table tracker-roster-table">
                    <thead>
                      <tr>
                        <th className="tracker-col-name">Full Name</th>
                        <th className="tracker-col-short">Status</th>
                        <th className="tracker-col-mid">Stage</th>
                        <th className="tracker-col-date">MKT Start Date</th>
                        <th className="tracker-col-date">EAD End Date</th>
                        <th className="tracker-col-mid">Marketing Location</th>
                        <th className="tracker-col-short">Nepal POC</th>
                        <th className="tracker-col-text">Current Location</th>
                        <th className="tracker-row-action">
                          <span className="tracker-sr-only">Open tracker</span>
                        </th>
                      </tr>
                    </thead>
                    <tbody>
                      {filteredCandidates.length === 0 ? (
                        <tr>
                          <td colSpan={9}>
                            <p className="tracker-empty tracker-empty-inline">
                              No candidates for this POC.
                            </p>
                          </td>
                        </tr>
                      ) : (
                        filteredCandidates.map((c) => (
                          <tr
                            key={c.email || c.name}
                            className={`tracker-candidate-row${c.email ? "" : " is-unlinked"}`}
                            tabIndex={c.email ? 0 : undefined}
                            role={c.email ? "button" : undefined}
                            aria-label={
                              c.email ? `Open ${c.name}'s tracker` : `${c.name} — no email on file`
                            }
                            title={
                              c.email
                                ? `Open ${c.name}'s tracker as they see it`
                                : "No personal email on Current_Market for this candidate"
                            }
                            onClick={() => {
                              if (c.email) openCandidate(c.email);
                            }}
                            onKeyDown={(e) => {
                              if (!c.email) return;
                              if (e.key === "Enter" || e.key === " ") {
                                e.preventDefault();
                                openCandidate(c.email);
                              }
                            }}
                          >
                            <td className="tracker-col-name tracker-cell-name" title={c.name}>
                              {c.name}
                              {c.email ? null : (
                                <span className="tracker-no-email">No email on file</span>
                              )}
                            </td>
                            <Cell className="tracker-col-short tracker-cell-value" value={c.visaStatus} />
                            <Cell className="tracker-col-mid tracker-cell-value" value={c.marketStage} />
                            <Cell className="tracker-col-date tracker-cell-date" value={c.mktStartDate} />
                            <Cell className="tracker-col-date tracker-cell-date" value={c.eadEndDate} />
                            <Cell className="tracker-col-mid tracker-cell-value" value={c.marketingLocation} />
                            <Cell className="tracker-col-short tracker-cell-value" value={c.nepalPoc} />
                            <Cell className="tracker-col-text tracker-cell-soft" value={c.currentLocation} />
                            <td className="tracker-row-action">
                              {c.email ? (
                                <button
                                  type="button"
                                  className="tracker-view-btn"
                                  tabIndex={-1}
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    openCandidate(c.email);
                                  }}
                                >
                                  View
                                  <IconChevron size={13} />
                                </button>
                              ) : (
                                <span className="tracker-muted">—</span>
                              )}
                            </td>
                          </tr>
                        ))
                      )}
                    </tbody>
                  </table>
                </div>
              </div>
            </section>
          ) : null}

          {!loading && showDetailEmpty ? (
            <section className="tracker-emptyview" aria-live="polite">
              <div className="tracker-emptyview-card">
                <span className="tracker-emptyview-icon">
                  <IconHistory size={20} />
                </span>
                <h2 className="tracker-emptyview-title">Nothing to show yet</h2>
                <p className="tracker-emptyview-text">
                  {data?.message ||
                    `No applies, interviews, or resumes are linked to ${
                      selectedEmail || "this candidate"
                    } yet.`}
                </p>
                <button
                  type="button"
                  className="tracker-btn tracker-back"
                  onClick={backToList}
                >
                  Back to candidates
                </button>
              </div>
            </section>
          ) : null}

          {!loading && showDetail ? (
            <>
              {/* Identity and the headline figures read as one instrument panel
                  rather than five separate floating cards. */}
              <section className="tracker-board" aria-label="Candidate summary">
                <div className="tracker-board-head">
                  <div className="tracker-board-id">
                    <p className="tracker-kicker">
                      {isAdmin ? "Viewing candidate" : "Signed in as"}
                    </p>
                    <h2 className="tracker-name">{profileName}</h2>
                    {data?.profile?.email ? (
                      <p className="tracker-email">{data.profile.email}</p>
                    ) : null}
                    {!isAdmin && data?.message ? (
                      <p className="tracker-banner">{data.message}</p>
                    ) : null}
                  </div>
                  <ApplyTallyPad
                    value={todayInput}
                    onChange={(next) => {
                      setTodayInput(next);
                      setApplySaveMsg("");
                    }}
                    onSave={() => void saveTodayApplies()}
                    saving={savingApplies}
                    message={applySaveMsg}
                  />
                </div>

                <dl className="tracker-board-stats">
                  <div className="tracker-stat">
                    <dt className="tracker-stat-label">Lifetime applies</dt>
                    <dd className="tracker-stat-value">{applies?.lifetime ?? 0}</dd>
                    <dd className="tracker-stat-note">All month tabs</dd>
                  </div>
                  <div className="tracker-stat">
                    <dt className="tracker-stat-label">Applies this month</dt>
                    <dd className="tracker-stat-value">{applies?.total ?? 0}</dd>
                    <dd className="tracker-stat-note">{monthTabLabel || "This month"}</dd>
                  </div>
                  <div className="tracker-stat">
                    <dt className="tracker-stat-label">Applies this week</dt>
                    <dd className="tracker-stat-value">{applies?.thisWeek ?? 0}</dd>
                    <dd className="tracker-stat-note">Mon–Sun (CST)</dd>
                  </div>
                  <div className="tracker-stat">
                    <dt className="tracker-stat-label">Companies</dt>
                    <dd className="tracker-stat-value">
                      {interviews?.companyCount ?? allCompanies.length}
                    </dd>
                    <dd className="tracker-stat-note">
                      {interviews?.total ?? 0} interviews · {interviews?.phoneCalls ?? 0} phone
                      calls · furthest {interviews?.deepestRound || "—"}
                    </dd>
                  </div>
                </dl>
              </section>

              <section className="tracker-charts" aria-label="Apply and phone-call activity">
                <AppliesAreaChart
                  className="tracker-chart-wide"
                  title="Lifetime applies"
                  subtitle="Monthly trend across every apply tab"
                  points={lifetimeBars}
                  emptyText="No apply counts on any month tab yet."
                />
                <AppliesAreaChart
                  title="Applies this month"
                  subtitle={selectedApplyMonth?.label || monthTabLabel || "Daily apply counts"}
                  points={monthBars}
                  actions={
                    <ChartPeriodSelect
                      value={selectedApplyMonth?.key || ""}
                      options={applyMonths}
                      onChange={setApplyMonthKey}
                      ariaLabel="Select apply month"
                    />
                  }
                />
                <AppliesAreaChart
                  title="Applies this week"
                  subtitle={selectedApplyWeek?.label || "Mon–Sun · CST"}
                  points={weekBars}
                  emptyText="No applies logged this week yet."
                  actions={
                    <ChartPeriodSelect
                      value={selectedApplyWeek?.key || ""}
                      options={applyWeeks}
                      onChange={setApplyWeekKey}
                      ariaLabel="Select apply week"
                    />
                  }
                />
                <AppliesAreaChart
                  title="Phone calls"
                  subtitle={`By month · older rows drop off the sheet after ${phoneCalls?.retentionDays ?? 7} days`}
                  unit="phone call"
                  unitPlural="phone calls"
                  points={phoneHasData ? phoneBars : []}
                  emptyText="—"
                />
                <OutcomePieChart
                  pending={data?.outcomes?.pending ?? 0}
                  rejected={data?.outcomes?.rejected ?? 0}
                  offered={data?.outcomes?.offered ?? 0}
                  emptyText="—"
                />
              </section>

              <section className="tracker-section" aria-label="Companies">
                <div className="tracker-section-head">
                  <h3 className="tracker-section-title">Companies</h3>
                  <div className="tracker-section-tools">
                    <p className="tracker-section-sub">
                      {companyFilter
                        ? `${companies.length} of ${allCompanies.length} companies`
                        : `${allCompanies.length} ${
                            allCompanies.length === 1 ? "company" : "companies"
                          } · ${interviews?.total ?? 0} interviews · ${
                            interviews?.phoneCalls ?? 0
                          } phone calls`}
                    </p>
                    {allCompanies.length > 8 ? (
                      <div className="tracker-search tracker-search-sm">
                        <IconSearch className="tracker-search-icon" />
                        <input
                          type="search"
                          className="tracker-filter"
                          placeholder="Filter companies"
                          value={companyFilter}
                          onChange={(e) => setCompanyFilter(e.target.value)}
                          aria-label="Filter companies"
                        />
                      </div>
                    ) : null}
                  </div>
                </div>
                {companies.length === 0 ? (
                  <p className="tracker-empty">
                    {companyFilter
                      ? "No companies match this filter."
                      : "No interviews or phone calls yet. Each company appears here as soon as an interview or phone call is booked or logged."}
                  </p>
                ) : (
                  <div className="tracker-table-card">
                    <div className="tracker-table-wrap">
                      <table className="tracker-table tracker-companies-table">
                        <thead>
                          <tr>
                            <th>Company</th>
                            <th>Type</th>
                            <th>Round reached</th>
                            <th>Interviews</th>
                            <th>Phone calls</th>
                            <th>Last date</th>
                            <th>Status</th>
                            <th>Resume</th>
                            <th>JD</th>
                          </tr>
                        </thead>
                        <tbody>
                          {companies.map((c, idx) => {
                            const status = companyStatus(c);
                            return (
                            <tr key={`${c.company}-${idx}`}>
                              <td className="tracker-company">{c.company || "—"}</td>
                              <td>{c.type || "Interview"}</td>
                              <td>{c.roundLabel || "—"}</td>
                              <td className="tracker-cell-count">{c.interviews}</td>
                              <td className="tracker-cell-count">{c.phoneCalls ?? 0}</td>
                              <td className="tracker-cell-strong">
                                {formatInterviewDate(c.lastDate, c.lastDateKey)}
                              </td>
                              <td className="tracker-status-cell">
                                {status ? (
                                  statusLabel(status)
                                ) : (
                                  <span className="tracker-doc-empty">—</span>
                                )}
                              </td>
                              <td className="tracker-doc-cell">
                                {docLink(c.resumeUrl, "Resume", "resume")}
                              </td>
                              <td className="tracker-doc-cell">
                                {docLink(c.jdUrl, "JD", "jd")}
                              </td>
                            </tr>
                            );
                          })}
                        </tbody>
                      </table>
                    </div>
                  </div>
                )}
              </section>
            </>
          ) : null}

          {!loading && data?.ok && !isAdmin && !data.matched ? (
            <p className="tracker-status">{data.message}</p>
          ) : null}
        </main>

        <SiteFooter variant="bar" />
      </div>
    </div>
  );
}
