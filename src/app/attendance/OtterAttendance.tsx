"use client";

import Link from "next/link";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useLiveAvailabilityPoll } from "@/lib/booking/useLiveAvailabilityPoll";
import type { AttendanceCandidate, AttendanceDay, AttendanceResponse } from "@/lib/attendance/types";
import { PRACTICE_MEET_URL } from "@/lib/content/classSchedules";
import { CalendarMonth } from "./CalendarMonth";
import { AttendanceBodySkeleton, rememberRole } from "./AttendanceSkeleton";
import "../book/phone-call/phone-call.css";
import "./attendance.css";

const POLL_MS = 60000;
const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

type SortKey = "name" | "days" | "time" | "last";
type DayState = "present" | "short" | "absent" | "future";

/** 75 → "1h 15m", 42 → "42m". */
function fmtMinutes(mins: number) {
  const m = Math.round(mins);
  if (m <= 0) return "0m";
  const h = Math.floor(m / 60);
  return h ? `${h}h ${String(m % 60).padStart(2, "0")}m` : `${m}m`;
}

function longDate(key: string) {
  const [y, m, d] = key.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString(undefined, { weekday: "long", month: "short", day: "numeric" });
}

function shortDate(key: string) {
  const [y, m, d] = key.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
}

function dayState(day: AttendanceDay | undefined, key: string, today: string): DayState {
  if (day?.present) return "present";
  if (day && day.minutes > 0) return "short";
  return key > today ? "future" : "absent";
}

function stateLabel(state: DayState, min: number) {
  if (state === "present") return "Present";
  if (state === "short") return `Under ${min} min`;
  if (state === "future") return "Upcoming";
  return "Absent";
}

/** Latest class day the candidate showed up (ticked or any recorded minutes). */
function lastAttended(c: AttendanceCandidate) {
  return Object.entries(c.days)
    .filter(([, d]) => d.present || d.minutes > 0)
    .map(([k]) => k)
    .sort()
    .pop();
}

function initials(name: string) {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((p) => p[0]?.toUpperCase())
    .join("");
}

function pct(n: number, of: number) {
  return of ? Math.min(100, Math.round((n / of) * 100)) : 0;
}

/* ------------------------------------------------------------ pieces */

function Meter({ value, tone = "brand" }: { value: number; tone?: "brand" | "good" }) {
  return (
    <span className={`at-meter is-${tone}`} aria-hidden>
      <span style={{ width: `${value}%` }} />
    </span>
  );
}

function Kpi({ label, value, of, hint, meter }: { label: string; value: string | number; of?: number; hint?: string; meter?: number }) {
  return (
    <div className="at-kpi">
      <span className="at-kpi-label">{label}</span>
      <span className="at-kpi-value">
        {value}
        {of !== undefined ? <small> / {of}</small> : null}
      </span>
      {meter !== undefined ? <Meter value={meter} /> : null}
      {hint ? <span className="at-kpi-hint">{hint}</span> : null}
    </div>
  );
}

function Avatar({ name, size = "md" }: { name: string; size?: "md" | "lg" }) {
  return (
    <span className={`at-avatar is-${size}`} aria-hidden>
      {initials(name) || "?"}
    </span>
  );
}

function StatusPill({ state, min }: { state: DayState; min: number }) {
  return <span className={`at-pill is-${state}`}>{stateLabel(state, min)}</span>;
}

/** Sessions + join/leave stretches for one candidate on one day. */
/** The two daily classes; a call is filed under the slot it falls in (AM / PM). */
const SLOTS = [
  { label: "11:30 AM", morning: true },
  { label: "2:00 PM", morning: false },
];

function DayBreakdown({ day, dateKey, today, min }: { day: AttendanceDay | undefined; dateKey: string; today: string; min: number }) {
  const state = dayState(day, dateKey, today);
  const forSlot = (morning: boolean) =>
    (day?.sessions ?? []).filter((x) => /AM/.test(x.session) === morning);

  return (
    <div className="at-day">
      <div className="at-day-head">
        <span className="at-day-date">{longDate(dateKey)}</span>
        <StatusPill state={state} min={min} />
      </div>
      {day?.sessions.length ? (
        <ul className="at-slots">
          {SLOTS.map((slot) => {
            const sessions = forSlot(slot.morning);
            const minutes = sessions.reduce((n, x) => n + x.minutes, 0);
            const joins = sessions.flatMap((x) => x.joins ?? []);
            return (
              <li key={slot.label} className={minutes > 0 ? undefined : "is-missed"}>
                <div className="at-slot-row">
                  <span className="at-slot-name">{slot.label} class</span>
                  <b>{minutes > 0 ? `${fmtMinutes(minutes)} joined` : "Not joined"}</b>
                </div>
                {joins.length ? (
                  <span className="at-slot-joins">
                    Joined{" "}
                    {joins.map((j) => `${j.from} – ${j.to}`).join(", ")}
                  </span>
                ) : null}
              </li>
            );
          })}
          <li className="at-slot-total">
            <span>Total time</span>
            <b>{fmtMinutes(day.minutes)}</b>
          </li>
        </ul>
      ) : (
        <p className="at-muted">
          {day?.present
            ? "Marked present by staff."
            : state === "future"
              ? "Classes at 11:30 AM and 2:00 PM CST."
              : "No time recorded for this day."}
        </p>
      )}
    </div>
  );
}

/* ------------------------------------------------------- one candidate */

const TICK = (
  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" aria-hidden>
    <path d="m5 12.5 4.5 4.5L19 7.5" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);
const CROSS = (
  <svg viewBox="0 0 24 24" width="14" height="14" fill="none" aria-hidden>
    <path d="M7 7l10 10M17 7 7 17" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" />
  </svg>
);

/**
 * One candidate's month: name + days attended on top, the calendar (green tick =
 * attended, red cross = absent), and the session/join detail for a clicked day.
 * Used for a candidate's own view and when staff open a candidate.
 */
function CandidateDetail({
  candidate,
  data,
  onBack,
}: {
  candidate: AttendanceCandidate;
  data: AttendanceResponse;
  /** Staff only: return to the candidate list. */
  onBack?: () => void;
}) {
  const today = data.today ?? "";
  const days = useMemo(() => data.days ?? [], [data.days]);
  const classDayKeys = useMemo(() => new Set(days.map((d) => d.key)), [days]);
  const soFar = days.filter((d) => d.key <= today).length;
  const min = data.minMinutes ?? 15;
  const monthNum = MONTHS.indexOf(data.month ?? "") + 1;
  const [picked, setPicked] = useState<string | null>(null);

  /** Month summary for the side panel. */
  const overview = useMemo(() => {
    const past = days.filter((d) => d.key <= today);
    const states = past.map((d) => dayState(candidate.days[d.key], d.key, today));
    let best = 0;
    let run = 0;
    for (const st of states) {
      run = st === "present" ? run + 1 : 0;
      best = Math.max(best, run);
    }
    return {
      present: states.filter((x) => x === "present").length,
      absent: states.filter((x) => x === "absent").length,
      short: states.filter((x) => x === "short").length,
      upcoming: days.length - past.length,
      best,
      recent: [...past].reverse().slice(0, 5),
    };
  }, [days, today, candidate.days]);

  /** Just a mark: green ✓ attended, red ✕ absent, minutes in grey when known. */
  const renderDay = (key: string) => {
    const day = candidate.days[key];
    const state = dayState(day, key, today);
    if (state === "future") return null;
    return (
      <span className={`mn-day is-${state}`}>
        <span className="mn-mark">{state === "absent" ? CROSS : state === "present" ? TICK : null}</span>
        {/* always reserve the minutes line so ✓, ✕ and the amber dot sit at the same height */}
        <span className={`mn-min${day && day.minutes > 0 ? "" : " is-empty"}`}>{day && day.minutes > 0 ? fmtMinutes(day.minutes) : "0m"}</span>
        <span className="at-sr">{stateLabel(state, min)}</span>
      </span>
    );
  };

  return (
    <section className="at-detail" aria-label={`${candidate.name} attendance`}>
      {onBack ? (
        <button type="button" className="at-back" onClick={onBack}>
          <svg viewBox="0 0 24 24" width="16" height="16" fill="none" aria-hidden>
            <path d="M15 5l-7 7 7 7" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          All candidates
        </button>
      ) : null}


      <div className="at-detail-body">
        <div className="at-card at-cal-card">
          <div className="at-card-head">
            <h3>
              {data.month} {data.year}
            </h3>
            <div className="at-legend">
              <span><i className="is-present" /> Present</span>
              <span><i className="is-short" /> Under {min} min</span>
              <span><i className="is-absent-red" /> Absent</span>
            </div>
          </div>
          {monthNum > 0 && data.year ? (
            <CalendarMonth
              year={data.year}
              month={monthNum}
              today={today}
              classDays={classDayKeys}
              selected={picked}
              onSelect={setPicked}
              renderDay={renderDay}
              dayClass={(key) => `is-${dayState(candidate.days[key], key, today)}`}
              label={(key) => stateLabel(dayState(candidate.days[key], key, today), min)}
            />
          ) : null}
        </div>

        <aside className="at-side-col">
          <div className="at-card at-pf">
            <div className="at-pf-head">
              <div className="at-pf-name">
                <h2>{candidate.name}</h2>
                {candidate.grade ? (
                  <span className="at-grade" title="Otter grade from the Data candidate sheet">
                    <small>Grade</small>
                    <b>{candidate.grade}</b>
                  </span>
                ) : null}
              </div>
              <p>Otter &amp; Pronunciation class</p>
            </div>

            <div className="at-pf-score">
              <div className="at-pf-big">
                <b>{candidate.presentDays}</b>
                <span>of {soFar}</span>
              </div>
              <p>Class days attended in {data.month}</p>
              <span className="at-pf-bar" aria-hidden>
                <span style={{ width: `${soFar ? Math.min(100, (candidate.presentDays / soFar) * 100) : 0}%` }} />
              </span>
            </div>


            {picked ? (
              <div className="at-pf-day">
                <DayBreakdown day={candidate.days[picked]} dateKey={picked} today={today} min={min} />
                <button type="button" className="at-pf-back" onClick={() => setPicked(null)}>
                  Back to {data.month} summary
                </button>
              </div>
            ) : (
              <>
                <section className="at-pf-sec">
                  <h3>{data.month} summary</h3>
                  <dl className="at-pf-list">
                    <div>
                      <dt>Days present</dt>
                      <dd>{overview.present}</dd>
                    </div>
                    <div>
                      <dt>Days absent</dt>
                      <dd>{overview.absent}</dd>
                    </div>
                    {overview.short ? (
                      <div>
                        <dt>Joined for under {min} minutes</dt>
                        <dd>{overview.short}</dd>
                      </div>
                    ) : null}
                    <div>
                      <dt>Class days remaining</dt>
                      <dd>{overview.upcoming}</dd>
                    </div>
                    <div>
                      <dt>Longest streak</dt>
                      <dd>
                        {overview.best} {overview.best === 1 ? "day" : "days"}
                      </dd>
                    </div>
                  </dl>
                </section>

                {overview.recent.length ? (
                  <section className="at-pf-sec">
                    <h3>Recent class days</h3>
                    <ul className="at-pf-recent">
                      {overview.recent.map((d) => {
                        const st = dayState(candidate.days[d.key], d.key, today);
                        const mins = candidate.days[d.key]?.minutes ?? 0;
                        return (
                          <li key={d.key}>
                            <button type="button" onClick={() => setPicked(d.key)}>
                              <span className={`at-ov-mark is-${st}`}>
                                {st === "present" ? TICK : st === "absent" ? CROSS : null}
                              </span>
                              <span>{shortDate(d.key)}</span>
                              <span className="at-pf-val">{mins > 0 ? fmtMinutes(mins) : stateLabel(st, min)}</span>
                            </button>
                          </li>
                        );
                      })}
                    </ul>
                  </section>
                ) : null}

                <p className="at-pf-hint">Select a day on the calendar to see the time spent in each session.</p>
              </>
            )}


            {/* The one Join class button on the page, at the foot of the card. */}
            <a className="at-pf-join" href={PRACTICE_MEET_URL} target="_blank" rel="noopener noreferrer">
              <svg viewBox="0 0 24 24" width="18" height="18" fill="none" aria-hidden>
                <rect x="2.5" y="6" width="13" height="12" rx="2.5" stroke="currentColor" strokeWidth="1.9" />
                <path
                  d="m15.5 10.2 5.1-3.1a.6.6 0 0 1 .9.5v8.8a.6.6 0 0 1-.9.5l-5.1-3.1"
                  stroke="currentColor"
                  strokeWidth="1.9"
                  strokeLinejoin="round"
                />
              </svg>
              Join class
            </a>
          </div>
        </aside>
      </div>
    </section>
  );
}

/* ---------------------------------------------------------------- page */


export function OtterAttendance() {
  const [data, setData] = useState<AttendanceResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [sort, setSort] = useState<SortKey>("days");
  const [openRow, setOpenRow] = useState<number | null>(null);
  // null = current month (the API's newest month tab). Admins can pick an earlier one.
  const monthRef = useRef<string | null>(null);
  const [month, setMonth] = useState<string | null>(null);
  // Phones: "Oct 2026" in the month picker so the title keeps its room.
  const [narrow, setNarrow] = useState(false);
  useEffect(() => {
    const mq = window.matchMedia("(max-width: 720px)");
    const sync = () => setNarrow(mq.matches);
    sync();
    mq.addEventListener("change", sync);
    return () => mq.removeEventListener("change", sync);
  }, []);
  // True from a month switch until that month arrives: show the skeleton, not the old month.
  const [switching, setSwitching] = useState(false);
  // Staff viewing one candidate: who to reopen once the new month loads (row numbers differ per tab).
  const reopen = useRef<{ email: string; name: string } | null>(null);

  // No month → the API serves the newest month tab (the current month).
  const load = useCallback(async (opts?: { silent?: boolean }) => {
    const wanted = monthRef.current;
    if (!opts?.silent) setLoading(true);
    try {
      const url = wanted ? `/api/otter-attendance?month=${encodeURIComponent(wanted)}` : "/api/otter-attendance";
      const res = await fetch(url, { cache: "no-store", credentials: "same-origin" });
      let json: AttendanceResponse;
      try {
        json = (await res.json()) as AttendanceResponse;
      } catch {
        json = { ok: false, error: res.ok ? "Invalid response" : `HTTP ${res.status}` };
      }
      if (!res.ok && json?.ok !== false) json = { ok: false, error: json?.error || `HTTP ${res.status}` };
      // Drop a response for a month the user has since switched away from.
      if (wanted !== monthRef.current) return;
      if (json.ok || !opts?.silent) setData(json);
      if (!opts?.silent) {
        const want = reopen.current;
        reopen.current = null;
        if (want && json.ok) {
          const match = (json.candidates ?? []).find(
            (c) =>
              (want.email && c.email.trim().toLowerCase() === want.email) ||
              (!want.email && c.name.trim().toLowerCase() === want.name)
          );
          setOpenRow(match ? match.row : null);
        }
        setSwitching(false);
      }
      if (json.ok) rememberRole(Boolean(json.isStaff));
    } catch (err) {
      if (!opts?.silent && wanted === monthRef.current) {
        setData({ ok: false, error: err instanceof Error ? err.message : String(err) });
      }
    } finally {
      if (!opts?.silent && wanted === monthRef.current) {
        setLoading(false);
        setSwitching(false);
      }
    }
  }, []);

  useLiveAvailabilityPoll(load, { intervalMs: POLL_MS });

  const switchMonth = (next: string) => {
    const current = openRow !== null ? data?.candidates?.find((c) => c.row === openRow) : undefined;
    reopen.current = current
      ? { email: current.email.trim().toLowerCase(), name: current.name.trim().toLowerCase() }
      : null;
    monthRef.current = next;
    setMonth(next);
    setSwitching(true);
    window.scrollTo({ top: 0 });
    void load();
  };

  const candidates = useMemo(() => data?.candidates ?? [], [data]);
  const days = useMemo(() => data?.days ?? [], [data]);
  const today = data?.today ?? "";
  const pastDays = days.filter((d) => d.key <= today);
  const classDaysSoFar = pastDays.length;
  const isStaff = Boolean(data?.isStaff);
  const mine = !isStaff ? candidates[0] : undefined;
  const opened = isStaff && openRow !== null ? candidates.find((c) => c.row === openRow) : undefined;
  const min = data?.minMinutes ?? 15;
  const listScroll = useRef(0);

  /** Staff: show one candidate as a full page; browser Back returns to the list. */
  const openCandidate = (row: number) => {
    listScroll.current = window.scrollY;
    window.history.pushState({ atCandidate: row }, "");
    setOpenRow(row);
    window.scrollTo({ top: 0 });
  };

  const closeCandidate = useCallback(() => {
    if (window.history.state?.atCandidate !== undefined) window.history.back();
    else setOpenRow(null);
  }, []);

  useEffect(() => {
    const onPop = () => setOpenRow(null);
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  useEffect(() => {
    if (openRow === null) {
      window.scrollTo({ top: listScroll.current });
      return;
    }
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && closeCandidate();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [openRow, closeCandidate]);

  const rows = useMemo(() => {
    const byName = (a: AttendanceCandidate, b: AttendanceCandidate) =>
      a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
    return [...candidates].sort((a, b) => {
      if (sort === "name") return byName(a, b);
      if (sort === "days") return b.presentDays - a.presentDays || b.totalMinutes - a.totalMinutes || byName(a, b);
      if (sort === "last") return (lastAttended(b) ?? "").localeCompare(lastAttended(a) ?? "") || byName(a, b);
      return b.totalMinutes - a.totalMinutes || byName(a, b);
    });
  }, [candidates, sort]);

  const sortHeader = (key: SortKey, label: string) => {
    const active = sort === key;
    return (
      <th aria-sort={active ? (key === "name" ? "ascending" : "descending") : "none"}>
        <button
          type="button"
          className={`at-sort${active ? " is-active" : ""}`}
          onClick={() => setSort(key)}
        >
          {label}
          <svg viewBox="0 0 10 10" width="9" height="9" aria-hidden>
            {active ? (
              <path d={key === "name" ? "M2 6.5 5 3.5l3 3" : "M2 3.5l3 3 3-3"} stroke="currentColor" strokeWidth="1.6" fill="none" />
            ) : (
              <path d="M2.5 4 5 1.8 7.5 4M2.5 6 5 8.2 7.5 6" stroke="currentColor" strokeWidth="1.3" fill="none" />
            )}
          </svg>
        </button>
      </th>
    );
  };

  return (
    <div className="at-root">
      <div className="pca-root at-shell">
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
                <h1>Otter Attendance</h1>
                <div className="page-subtitle">Mon–Fri · 11:30 AM &amp; 2:00 PM CST</div>
              </div>
              <div className="actions">
                {data?.ok && data.months?.length ? (
                  <label className="at-month-pick">
                    <span className="at-sr">Month</span>
                    <select value={month ?? data.month ?? ""} onChange={(e) => switchMonth(e.target.value)}>
                      {data.months.map((m) => (
                        <option key={m.key} value={m.key}>
                          {narrow ? m.label.replace(/^([A-Za-z]{3})[A-Za-z]*/, "$1") : m.label}
                        </option>
                      ))}
                    </select>
                    <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden>
                      <path d="M2 3.5l3 3 3-3" stroke="currentColor" strokeWidth="1.6" fill="none" />
                    </svg>
                  </label>
                ) : data?.month ? (
                  <span className="at-month-badge" aria-label={`${data.month} ${data.year}`}>
                    <span className="at-month-badge-top">{data.month.slice(0, 3)}</span>
                    <span className="at-month-badge-year">{data.year}</span>
                  </span>
                ) : null}
              </div>
            </div>
          </div>
          <div className="red-rule" />
        </header>
      </div>

      <main className={`at-main${loading && data ? " is-refreshing" : ""}`}>
        {(loading && !data) || switching ? (
          <AttendanceBodySkeleton view={switching && openRow !== null ? "calendar" : undefined} />
        ) : null}

        {switching ? null : data && !data.ok ? (
          <div className="at-card at-message">
            <b>Couldn&apos;t load attendance</b>
            {data.error ? <span>{data.error}</span> : null}
            <button type="button" className="at-btn" onClick={() => void load()}>
              Try again
            </button>
          </div>
        ) : null}

        {/* Candidate: only their own record */}
        {!switching && data?.ok && !isStaff ? (
          mine ? (
            <CandidateDetail candidate={mine} data={data} />
          ) : (
            <div className="at-card at-message">
              {month && data.months?.length && month !== data.months[0].key ? (
                <>
                  <b>No attendance for you in {data.month}</b>
                  <span>You weren&apos;t on the {data.month} class roster. Pick another month above.</span>
                </>
              ) : (
                <>
                  <b>You&apos;re not on the {data.month ?? "class"} roster yet</b>
                  <span>Ask the Cubic team to add the email you use to sign in to this site.</span>
                </>
              )}
            </div>
          )
        ) : null}

        {/* Staff: whole class */}
        {!switching && data?.ok && isStaff && opened ? (
          <CandidateDetail key={opened.row} candidate={opened} data={data} onBack={closeCandidate} />
        ) : null}

        {!switching && data?.ok && isStaff && !opened ? (
          <div className="at-stack">
            <section className="at-card">
              <div className="at-card-head">
                <div>
                  <h3>Candidates</h3>
                  <p>{`${candidates.length} on the ${data.month} roster`}</p>
                </div>
              </div>

              <div className="at-table-scroll">
                <table className="at-table">
                  <thead>
                    <tr>
                      {sortHeader("name", "Candidate")}
                      {sortHeader("days", "Attendance")}
                      {sortHeader("time", "Time in class")}
                      {sortHeader("last", "Last attended")}
                    </tr>
                  </thead>
                  <tbody>
                    {!rows.length ? (
                      <tr>
                        <td colSpan={4} className="at-table-empty">
                          No candidates on this month’s roster yet.
                        </td>
                      </tr>
                    ) : null}
                    {rows.map((c) => {
                      const share = pct(c.presentDays, classDaysSoFar);
                      const last = lastAttended(c);
                      return (
                        <tr
                          key={c.row}
                          tabIndex={0}
                          className={openRow === c.row ? "is-selected" : undefined}
                          onClick={() => openCandidate(c.row)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter" || e.key === " ") {
                              e.preventDefault();
                              openCandidate(c.row);
                            }
                          }}
                        >
                          <td>
                            <span className="at-person">
                              <Avatar name={c.name} />
                              <span className="at-person-text">
                                <span className="at-name">
                                  {c.name}
                                  {c.grade ? (
                                    <span className="at-grade-chip" title="Otter grade">
                                      Grade <b>{c.grade}</b>
                                    </span>
                                  ) : null}
                                </span>
                                {c.email ? <span className="at-email">{c.email}</span> : null}
                              </span>
                            </span>
                          </td>
                              <td>
                                <span className="at-rate">
                                  <span className="at-rate-text">
                                    <b>{c.presentDays}</b>
                                    <span> / {classDaysSoFar} days</span>
                                  </span>
                                  <Meter value={share} tone="good" />
                                </span>
                              </td>
                              <td className="at-strong">{c.totalMinutes > 0 ? fmtMinutes(c.totalMinutes) : <span className="at-cell-muted">—</span>}</td>
                              <td>{last ? shortDate(last) : <span className="at-cell-muted">Not yet</span>}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            </section>
          </div>
        ) : null}
      </main>
    </div>
  );
}
