"use client";

import { useCallback, useRef, useState, type ReactNode } from "react";

export type CompanyPoint = {
  company: string;
  total: number;
  data: number;
  java: number;
  other: number;
};

type Tip = { x: number; y: number; body: ReactNode } | null;

/** Track symbols: a SQL database for Data, a coffee cup for Java. Colour comes from currentColor. */
export function TrackIcon({ track, size = 16 }: { track: "data" | "java"; size?: number }) {
  if (track === "data") {
    return (
      <svg className="ci-track-icon is-data" width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden>
        <ellipse cx="12" cy="5.5" rx="7.5" ry="3" fill="currentColor" />
        <path
          d="M4.5 5.5v13c0 1.66 3.36 3 7.5 3s7.5-1.34 7.5-3v-13"
          stroke="currentColor"
          strokeWidth="1.8"
        />
        <path d="M4.5 12c0 1.66 3.36 3 7.5 3s7.5-1.34 7.5-3" stroke="currentColor" strokeWidth="1.8" />
      </svg>
    );
  }
  return (
    <svg className="ci-track-icon is-java" width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden>
      <path
        d="M9 2.5c-1 1.2 1 2.2 0 3.5M12.5 2.5c-1 1.2 1 2.2 0 3.5"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
      />
      <path
        d="M4.5 9h12v5.5a5 5 0 0 1-5 5h-2a5 5 0 0 1-5-5V9z"
        fill="currentColor"
      />
      <path d="M16.5 10.5h1.2a2.3 2.3 0 0 1 0 4.6h-1.6" stroke="currentColor" strokeWidth="1.7" />
      <path d="M3.5 21.5h15" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
    </svg>
  );
}

export function LegendKey({ tone, label }: { tone: "data" | "java" | "other"; label: string }) {
  return (
    <span className="ci-legend-item">
      <i className={`ci-key ci-key-${tone}`} aria-hidden />
      {label}
    </span>
  );
}

/** Top companies by interviews received, each bar split Data / Java / other. */
export function CompanyBars({
  rows,
  onSelect,
}: {
  rows: CompanyPoint[];
  onSelect: (company: string) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [tip, setTip] = useState<Tip>(null);
  const max = Math.max(1, ...rows.map((r) => r.total));
  const hasData = rows.some((r) => r.data > 0);
  const hasJava = rows.some((r) => r.java > 0);
  const hasOther = rows.some((r) => r.other > 0);

  const show = useCallback((e: React.PointerEvent | React.FocusEvent, body: ReactNode) => {
    const box = ref.current?.getBoundingClientRect();
    if (!box) return;
    if ("clientX" in e) {
      setTip({ x: e.clientX - box.left, y: e.clientY - box.top, body });
    } else {
      const r = (e.target as HTMLElement).getBoundingClientRect();
      setTip({ x: r.left + r.width / 2 - box.left, y: r.top - box.top, body });
    }
  }, []);
  const hide = useCallback(() => setTip(null), []);

  return (
    <section className="day-card ci-panel ci-top" ref={ref}>
      <div className="day-head">
        <div className="day-name">Top companies</div>
        <div className="day-sum ci-legend">
          {hasData ? <LegendKey tone="data" label="Data" /> : null}
          {hasJava ? <LegendKey tone="java" label="Java" /> : null}
          {hasOther ? <LegendKey tone="other" label="Other" /> : null}
        </div>
      </div>

      {!rows.length ? (
        <p className="ci-panel-empty">No companies match these filters.</p>
      ) : (
        <ol className="ci-hbars">
          {rows.map((r, i) => {
            const body = (
              <>
                <strong>
                  {r.total} {r.total === 1 ? "interview" : "interviews"}
                </strong>
                <span>{r.company}</span>
                <span className="ci-tip-rows">
                  {r.data ? <em><i className="ci-tip-key ci-key-data" />Data {r.data}</em> : null}
                  {r.java ? <em><i className="ci-tip-key ci-key-java" />Java {r.java}</em> : null}
                  {r.other ? <em><i className="ci-tip-key ci-key-other" />Other {r.other}</em> : null}
                </span>
              </>
            );
            return (
              <li key={r.company}>
                <button
                  type="button"
                  className="ci-hbar-row"
                  onClick={() => onSelect(r.company)}
                  onPointerMove={(e) => show(e, body)}
                  onPointerLeave={hide}
                  onFocus={(e) => show(e, body)}
                  onBlur={hide}
                  aria-label={`${r.company}: ${r.total} interviews, ${r.data} data, ${r.java} java`}
                >
                  <span className="ci-hbar-rank">{i + 1}</span>
                  <span className="ci-hbar-name">{r.company}</span>
                  <span className="ci-hbar-track">
                    <span className="ci-hbar-stack" style={{ width: `${(r.total / max) * 100}%` }}>
                      {r.data ? <span className="ci-seg ci-seg-data" style={{ flexGrow: r.data }} /> : null}
                      {r.java ? <span className="ci-seg ci-seg-java" style={{ flexGrow: r.java }} /> : null}
                      {r.other ? <span className="ci-seg ci-seg-other" style={{ flexGrow: r.other }} /> : null}
                    </span>
                    <span className="ci-hbar-value">{r.total}</span>
                  </span>
                </button>
              </li>
            );
          })}
        </ol>
      )}

      {tip ? (
        <div
          className="ci-tip"
          role="tooltip"
          style={{ left: tip.x, top: tip.y }}
          data-flip={ref.current && tip.x > ref.current.clientWidth - 180 ? "1" : undefined}
        >
          {tip.body}
        </div>
      ) : null}
    </section>
  );
}
