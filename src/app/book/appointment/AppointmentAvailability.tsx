"use client";

import Link from "next/link";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { DoNotApplyPanel } from "@/components/modals/DoNotApplyPanel";
import { SiteFooter } from "@/components/ui/SiteFooter";
import { StatusToast, useStatusToast } from "@/components/ui/StatusToast";
import type { AppointmentDay } from "@/lib/appointment/types";
import { useLiveAvailabilityPoll } from "@/lib/booking/useLiveAvailabilityPoll";
import { reserveSlot } from "@/lib/booking/useSlotHold";
import { minsToCstLabel, minsToTimeStr, parseUsZone, ZONE_OFFSET_FROM_CST } from "@/lib/ui/clock";
import {
  AppointmentBookModal,
  type BookPrefill,
} from "./AppointmentBookModal";
import "../phone-call/phone-call.css";

type ApiResponse = {
  ok: boolean;
  error?: string;
  days?: AppointmentDay[];
  generatedAt?: string;
  signature?: string;
};

function formatTimeInZone(start: number, end: number, zone: string) {
  const off = ZONE_OFFSET_FROM_CST[parseUsZone(zone)];
  const label = parseUsZone(zone);
  return `${minsToTimeStr(start + off)} – ${minsToTimeStr(end + off)} ${label}`;
}

const PAGE_SIZE = 5;
const POLL_MS = 15000;

function daySum(day: AppointmentDay) {
  const bookable = day.blocks.filter((b) => b.type !== "emergency");
  const total = bookable.reduce((s, b) => s + (b.capacity || 1), 0);
  const available = bookable.reduce((s, b) => {
    if (b.type === "full") return s;
    return s + Math.max(0, b.minFree || 0);
  }, 0);
  const sumClass =
    available === 0 ? "sum-none" : available < total ? "sum-some" : "sum-all";
  return { available, total, sumClass };
}

function SkeletonDay({ rows = 8 }: { rows?: number }) {
  return (
    <section className="day-card skeleton-card" aria-hidden>
      <div className="day-head skeleton-head">
        <span className="skel skel-title" />
        <span className="skel skel-sum" />
      </div>
      <div className="slot-head" aria-hidden>
        <span>Time</span>
        <span>Slot availability</span>
        <span />
      </div>
      {Array.from({ length: rows }).map((_, i) => (
        <div className="row skeleton-row" key={i}>
          <span className="skel skel-time" />
          <span className="skel skel-chip" />
          <span className="skel skel-btn" />
        </div>
      ))}
    </section>
  );
}

function prettyDate(dateIso: string) {
  const [y, m, d] = dateIso.split("-").map(Number);
  if (!y || !m || !d) return dateIso;
  const months = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
  ];
  return `${months[m - 1]} ${d}, ${y}`;
}

export function AppointmentAvailability() {
  const [data, setData] = useState<ApiResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [zone, setZone] = useState("CST");
  const [rulesOpen, setRulesOpen] = useState(false);
  const [bookPrefill, setBookPrefill] = useState<BookPrefill | null>(null);
  const [page, setPage] = useState(0);
  const [dnaOpen, setDnaOpen] = useState(false);
  const [reserving, setReserving] = useState<string | null>(null);
  const signatureRef = useRef<string | null>(null);
  const bookOpenRef = useRef(false);
  const pendingRefreshRef = useRef(false);
  const reservingRef = useRef(false);
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const status = useStatusToast();

  bookOpenRef.current = !!bookPrefill;

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
        const res = await fetch("/api/appointment-availability", { cache: "no-store" });
        const json = (await res.json()) as ApiResponse;
        applyPayload(json, opts);
      } catch (err) {
        if (!opts?.silent) {
          setData({
            ok: false,
            error: err instanceof Error ? err.message : String(err),
          });
        }
      } finally {
        if (!opts?.silent) setLoading(false);
      }
    },
    [applyPayload]
  );

  const openBook = useCallback(
    async (dateIso: string, startMin: number | null, emergency: boolean) => {
      const picked = parseUsZone(zone);
      const off = ZONE_OFFSET_FROM_CST[picked];
      let subtitle = `Booking for ${prettyDate(dateIso)}`;
      let meetingTime = "";
      if (startMin !== null) {
        const local = `${minsToTimeStr(startMin + off)} ${picked}`;
        subtitle = emergency
          ? `Emergency booking for ${prettyDate(dateIso)} · ${local}`
          : `Booking for ${prettyDate(dateIso)} · ${local}`;
        meetingTime = minsToCstLabel(startMin);
      }
      const base = {
        dateIso,
        startMin,
        meetingTime,
        meetingDuration: "1 Hr",
        emergency,
        subtitle,
        timeZone: picked,
      };

      // Emergency slots have no capacity limit — open straight away.
      if (emergency || startMin === null) {
        setBookPrefill(base);
        return;
      }

      // Reserve the slot BEFORE opening the form. If the last seat was just
      // taken, the form never opens — the candidate only gets a toast.
      if (reservingRef.current) return;
      const key = `${dateIso}-${startMin}`;
      reservingRef.current = true;
      setReserving(key);
      const r = await reserveSlot("interview", dateIso, startMin, 60);
      reservingRef.current = false;
      setReserving(null);

      if (!r.ok || !r.holdId) {
        status.show(
          "error",
          r.error || "This time is already being booked. Choose another slot.",
          "Slot unavailable"
        );
        void load({ silent: true });
        return;
      }

      setBookPrefill({ ...base, holdId: r.holdId, holdExpiresAt: r.expiresAt });
    },
    [zone, status.show, load]
  );

  useLiveAvailabilityPoll(load, {
    intervalMs: POLL_MS,
    isPaused: () => bookOpenRef.current,
    onPausedTick: () => {
      pendingRefreshRef.current = true;
    },
  });

  const days = useMemo(() => data?.days || [], [data]);
  const pageCount = Math.max(1, Math.ceil(days.length / PAGE_SIZE));
  const safePage = Math.min(Math.max(page, 0), pageCount - 1);
  const pageDays = useMemo(() => {
    const from = safePage * PAGE_SIZE;
    return days.slice(from, from + PAGE_SIZE);
  }, [days, safePage]);

  useEffect(() => {
    if (page !== safePage) setPage(safePage);
  }, [page, safePage]);

  const changePage = (delta: number) => {
    setPage((p) => Math.min(Math.max(p + delta, 0), pageCount - 1));
    wrapRef.current?.scrollIntoView({ block: "start", behavior: "smooth" });
  };

  const pagerRange =
    pageDays.length > 0
      ? `${pageDays[0].label} – ${pageDays[pageDays.length - 1].label}`
      : "";

  return (
    <div className="pca-root">
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
              <h1>Appointment Availability</h1>
            </div>
            <div className="actions">
              <label className="tz-picker">
                <span>Timezone</span>
                <select
                  value={zone}
                  onChange={(e) => setZone(e.target.value)}
                  aria-label="Timezone"
                >
                  <option value="EST">EST (Eastern)</option>
                  <option value="CST">CST (Central)</option>
                  <option value="MST">MST (Mountain)</option>
                  <option value="PST">PST (Pacific)</option>
                </select>
              </label>
              <button type="button" className="ghost-btn" onClick={() => setRulesOpen(true)}>
                Rules
              </button>
            </div>
          </div>
        </div>
        <div className="red-rule" />
      </header>

      <div className="pca-body" ref={wrapRef}>
      <main className="wrap">
        {loading && !days.length ? (
          <div className="skeleton-list" aria-busy="true" aria-label="Loading availability">
            <SkeletonDay rows={9} />
            <SkeletonDay rows={9} />
            <SkeletonDay rows={9} />
          </div>
        ) : null}

        {data && !data.ok ? (
          <div className="empty error">
            Could not load availability: {data.error || "Unknown error"}
          </div>
        ) : null}

        {pageDays.map((day) => {
          const sum = daySum(day);
          return (
            <section key={day.dateIso} className="day-card">
              <div className="day-head">
                <span className="day-name">{day.label}</span>
                <span className={`day-sum ${sum.sumClass}`}>
                  {sum.available}/{sum.total} seats open
                </span>
              </div>
              <div className="slot-head" aria-hidden>
                <span>Time</span>
                <span>Slot availability</span>
                <span />
              </div>
              {day.blocks.map((block) => {
                if (block.type === "emergency") {
                  return (
                    <div className="row" key={`${day.dateIso}-emergency`}>
                      <span className="time time-emergency">Emergency Slot</span>
                      <span className="chip">-/-</span>
                      <button
                        type="button"
                        className="book-btn"
                        onClick={() => openBook(day.dateIso, null, true)}
                      >
                        Book
                      </button>
                    </div>
                  );
                }

                const isFull = block.type === "full" || block.minFree <= 0;
                const free = isFull ? 0 : block.minFree;
                return (
                  <div
                    className={`row${isFull ? " row-muted" : ""}`}
                    key={`${day.dateIso}-${block.start}`}
                  >
                    <span className={`time${isFull ? " muted" : ""}`}>
                      {formatTimeInZone(block.start, block.end, zone)}
                    </span>
                    <span className="chip">
                      {free}/{block.capacity}
                    </span>
                    {isFull ? (
                      <span className="full-label">All slots booked</span>
                    ) : (
                      <button
                        type="button"
                        className="book-btn"
                        disabled={reserving === `${day.dateIso}-${block.start}`}
                        onClick={() => openBook(day.dateIso, block.start, false)}
                      >
                        {reserving === `${day.dateIso}-${block.start}`
                          ? "Reserving…"
                          : "Book"}
                      </button>
                    )}
                  </div>
                );
              })}
            </section>
          );
        })}

        {!loading && data?.ok && days.length === 0 ? (
          <div className="empty">No availability remaining in this window.</div>
        ) : null}

        {days.length > PAGE_SIZE ? (
          <div className="pager" role="navigation" aria-label="Day pages">
            <button
              type="button"
              className="pager-btn"
              onClick={() => changePage(-1)}
              disabled={safePage === 0}
            >
              ← Previous
            </button>
            <div className="pager-info">
              Page {safePage + 1} of {pageCount}
              {pagerRange ? (
                <>
                  <br />
                  <span className="pager-range">{pagerRange}</span>
                </>
              ) : null}
            </div>
            <button
              type="button"
              className="pager-btn"
              onClick={() => changePage(1)}
              disabled={safePage >= pageCount - 1}
            >
              Next →
            </button>
          </div>
        ) : null}
      </main>
      <SiteFooter variant="bar" />
      </div>

      {rulesOpen ? (
        <div className="rules-backdrop" role="presentation" onClick={() => setRulesOpen(false)}>
          <div
            className="rules-panel"
            role="dialog"
            aria-modal="true"
            aria-label="Scheduling rules"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="rules-head">
              <h2>Scheduling rules</h2>
              <button type="button" aria-label="Close" onClick={() => setRulesOpen(false)}>
                ✕
              </button>
            </div>
            <ul className="rules-list">
              <li>
                <strong>Office hours</strong> — Monday–Thursday 8:00 AM – 5:00 PM CST; Friday
                8:00 AM – 3:00 PM CST.
              </li>
              <li>
                <strong>Lunch</strong> — 12:00 PM – 1:00 PM CST is never scheduled.
              </li>
              <li>
                <strong>Capacity</strong> — up to 3 meetings can run at the same time.
              </li>
              <li>
                <strong>Source of truth</strong> — open/full from Cubic Interview Sheet month
                tabs (Date / Time / Duration).
              </li>
              <li>
                <strong>Emergency slot</strong> — always open after hours; team Accepts or
                Declines on the sheet.
              </li>
            </ul>
          </div>
        </div>
      ) : null}

      <AppointmentBookModal
        open={!!bookPrefill}
        prefill={bookPrefill}
        onClose={() => {
          setBookPrefill(null);
          if (pendingRefreshRef.current) {
            pendingRefreshRef.current = false;
            void load({ silent: true });
          }
        }}
        onSuccess={() => {
          pendingRefreshRef.current = true;
        }}
      />

      {!bookPrefill ? (
        <button
          type="button"
          className="pca-page-dna-fab"
          aria-label="Do Not Apply list"
          title="Do Not Apply"
          onClick={() => setDnaOpen(true)}
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="#ffffff"
            strokeWidth="2.8"
            strokeLinecap="round"
            aria-hidden
          >
            <circle cx="12" cy="12" r="9" />
            <path d="M16.5 7.5L7.5 16.5" />
          </svg>
        </button>
      ) : null}

      <DoNotApplyPanel open={dnaOpen} onClose={() => setDnaOpen(false)} />

      <StatusToast toast={status.toast} onDismiss={status.dismiss} />
    </div>
  );
}
