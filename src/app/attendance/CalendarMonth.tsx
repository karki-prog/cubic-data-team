"use client";

import { useMemo, type ReactNode } from "react";

const WEEK = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

function key(d: Date) {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/**
 * Month calendar: Sun–Sat grid with neighbouring months' days faded and today
 * marked. Only `classDays` are clickable; each one renders `renderDay(key)` and
 * gets the extra class name from `dayClass(key)` (used for status tints).
 */
export function CalendarMonth({
  year,
  month,
  today,
  classDays,
  selected,
  onSelect,
  renderDay,
  dayClass,
  label,
}: {
  year: number;
  /** 1–12 */
  month: number;
  today: string;
  classDays: Set<string>;
  selected: string | null;
  onSelect: (key: string | null) => void;
  renderDay: (dayKey: string) => ReactNode;
  dayClass?: (dayKey: string) => string | undefined;
  /** Accessible summary for a class day button. */
  label?: (dayKey: string) => string;
}) {
  const cells = useMemo(() => {
    const first = new Date(year, month - 1, 1);
    const start = new Date(first);
    start.setDate(1 - first.getDay());
    const lastOfMonth = new Date(year, month, 0);
    const weeks = Math.ceil((first.getDay() + lastOfMonth.getDate()) / 7);
    return Array.from({ length: weeks * 7 }, (_, i) => {
      const d = new Date(start);
      d.setDate(start.getDate() + i);
      return { date: d, key: key(d), inMonth: d.getMonth() === month - 1, weekend: d.getDay() === 0 || d.getDay() === 6 };
    });
  }, [year, month]);

  return (
    <div className="cal" role="grid" aria-label="Month calendar">
      <div className="cal-head" role="row">
        {WEEK.map((w) => (
          <span key={w} role="columnheader">
            {w}
          </span>
        ))}
      </div>
      <div className="cal-grid">
        {cells.map((c) => {
          const isClass = c.inMonth && classDays.has(c.key);
          const isToday = c.key === today;
          const active = selected === c.key;
          const extra = isClass ? dayClass?.(c.key) : undefined;
          const cls = [
            "cal-cell",
            !c.inMonth && "is-out",
            c.weekend && "is-weekend",
            isToday && "is-today",
            active && "is-active",
            isClass && "is-class",
            extra,
          ]
            .filter(Boolean)
            .join(" ");
          const body = (
            <>
              <span className="cal-num">{c.date.getDate()}</span>
              {isClass ? <span className="cal-body">{renderDay(c.key)}</span> : null}
            </>
          );
          return isClass ? (
            <button
              key={c.key}
              type="button"
              role="gridcell"
              aria-selected={active}
              aria-label={`${c.date.toDateString()}${label ? `: ${label(c.key)}` : ""}`}
              className={cls}
              onClick={() => onSelect(active ? null : c.key)}
            >
              {body}
            </button>
          ) : (
            <div key={c.key} role="gridcell" className={cls}>
              {body}
            </div>
          );
        })}
      </div>
    </div>
  );
}

/** Calendar-shaped skeleton for the loading state. */
export function CalendarSkeleton({ weeks = 5 }: { weeks?: number }) {
  return (
    <div className="cal is-skeleton" aria-hidden>
      <div className="cal-head">
        {WEEK.map((w) => (
          <span key={w}>
            <span className="cubic-skel" style={{ width: 24, height: 8 }} />
          </span>
        ))}
      </div>
      <div className="cal-grid">
        {Array.from({ length: weeks * 7 }).map((_, i) => {
          const weekend = i % 7 === 0 || i % 7 === 6;
          return (
            <div key={i} className={`cal-cell${weekend ? " is-weekend" : ""}`}>
              <span className="cubic-skel" style={{ width: 14, height: 10 }} />
              {!weekend ? <span className="cubic-skel" style={{ width: "70%", height: 10, marginTop: "auto" }} /> : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}
