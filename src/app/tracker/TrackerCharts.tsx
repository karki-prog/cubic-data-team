"use client";

import { useEffect, useState, type ReactNode } from "react";

/** Phones: draw charts on a narrower canvas so text stays readable and nothing is empty. */
function useNarrow() {
  const [narrow, setNarrow] = useState(false);
  useEffect(() => {
    const mq = window.matchMedia("(max-width: 640px)");
    const sync = () => setNarrow(mq.matches);
    sync();
    mq.addEventListener("change", sync);
    return () => mq.removeEventListener("change", sync);
  }, []);
  return narrow;
}

type BarPoint = {
  label: string;
  value: number;
  active?: boolean;
  title?: string;
};

function ChartFrame({
  title,
  subtitle,
  className,
  actions,
  children,
}: {
  title: string;
  subtitle?: string;
  className?: string;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <article className={`tracker-chart-card${className ? ` ${className}` : ""}`}>
      <header className="tracker-chart-head">
        <div className="tracker-chart-head-copy">
          <h3 className="tracker-chart-title">{title}</h3>
          {subtitle ? <p className="tracker-chart-sub">{subtitle}</p> : null}
        </div>
        {actions ? <div className="tracker-chart-actions">{actions}</div> : null}
      </header>
      {children}
    </article>
  );
}

export type PeriodOption = {
  key: string;
  label: string;
  total?: number;
  isCurrent?: boolean;
  byDay?: { day: number; label: string; count: number; isToday?: boolean }[];
};

export function ChartPeriodSelect({
  value,
  options,
  onChange,
  ariaLabel,
}: {
  value: string;
  options: PeriodOption[];
  onChange: (key: string) => void;
  ariaLabel: string;
}) {
  if (options.length === 0) return null;
  return (
    <select
      className="tracker-chart-select"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      aria-label={ariaLabel}
    >
      {options.map((o) => (
        <option key={o.key} value={o.key}>
          {o.label}
          {typeof o.total === "number" ? ` · ${o.total}` : ""}
        </option>
      ))}
    </select>
  );
}

/** Round the y-max up to a 1 / 2 / 5 × 10^n scale so axis labels stay even. */
function niceCeiling(value: number): number {
  if (value <= 0) return 4;
  const exp = Math.floor(Math.log10(value));
  const mag = 10 ** exp;
  const n = value / mag;
  const nice = n <= 1 ? 1 : n <= 2 ? 2 : n <= 5 ? 5 : 10;
  return nice * mag;
}

function yTicks(max: number, steps = 4): number[] {
  return Array.from({ length: steps + 1 }, (_, i) => (max * i) / steps);
}

function showXLabel(index: number, total: number, label: string): boolean {
  const day = Number(label);
  const numeric = Number.isFinite(day);
  if (!numeric && total <= 16) return true;
  if (total <= 8) return true;
  if (index === 0 || index === total - 1) return true;
  if (numeric) return day === 1 || day % 5 === 0;
  return index % Math.ceil(total / 6) === 0;
}

/** Time series of apply counts — a line reads trend better than a crowded bar row. */
export function AppliesAreaChart({
  title,
  subtitle,
  points,
  unit = "apply",
  unitPlural = "applies",
  emptyText = "No apply counts on any month tab yet.",
  className,
  variant = "area",
  actions,
}: {
  title: string;
  subtitle?: string;
  points: BarPoint[];
  unit?: string;
  unitPlural?: string;
  emptyText?: string;
  className?: string;
  variant?: "area" | "bar";
  actions?: ReactNode;
}) {
  const rawMax = Math.max(0, ...points.map((p) => p.value));
  const max = niceCeiling(rawMax);
  const total = points.reduce((sum, p) => sum + p.value, 0);
  const ticks = yTicks(max);
  const unitLabel = total === 1 ? unit : unitPlural;
  const isBar = variant === "bar";
  const narrow = useNarrow();
  // Many points on a phone: label every 5th one and skip per-bar values.
  const crowded = narrow && points.length > 10;
  const labelAt = (i: number, label: string) =>
    crowded ? i === 0 || i === points.length - 1 || (i + 1) % 5 === 0 : showXLabel(i, points.length, label);

  const width = narrow ? 480 : 960;
  const chartH = narrow ? 240 : isBar ? 300 : 280;
  const padX = narrow ? 44 : 56;
  const padRight = 24;
  const padTop = isBar ? 40 : 32;
  const padBottom = 48;
  const plotW = width - padX - padRight;
  const step = points.length > 1 ? plotW / (points.length - 1) : plotW;
  const slot = points.length > 0 ? plotW / points.length : plotW;
  const barW = Math.max(10, Math.min(42, slot * 0.72));
  const dense = points.length > 14;
  const coords = points.map((p, i) => {
    const x = isBar
      ? padX + i * slot + slot / 2
      : points.length === 1
        ? padX + plotW / 2
        : padX + i * step;
    const y = padTop + chartH - (p.value / max) * chartH;
    return { ...p, x, y };
  });
  const baseline = padTop + chartH;
  const lineD = coords
    .map((p, i) => `${i === 0 ? "M" : "L"} ${p.x.toFixed(1)} ${p.y.toFixed(1)}`)
    .join(" ");
  const areaD =
    coords.length === 0
      ? ""
      : `${lineD} L ${coords[coords.length - 1].x.toFixed(1)} ${baseline} L ${coords[0].x.toFixed(1)} ${baseline} Z`;

  return (
    <ChartFrame
      className={className}
      title={title}
      subtitle={[subtitle, `${total} ${unitLabel}`].filter(Boolean).join(" · ")}
      actions={actions}
    >
      {points.length === 0 ? (
        <p className={emptyText === "—" ? "tracker-chart-dash" : "tracker-empty"}>{emptyText}</p>
      ) : (
        <div className="tracker-chart-plot">
          <svg
            className={`tracker-bar-svg${narrow ? " is-narrow" : ""}`}
            viewBox={`0 0 ${width} ${chartH + padTop + padBottom}`}
            preserveAspectRatio="xMidYMid meet"
            role="img"
            aria-label={`${title} — ${total} total`}
          >
            {ticks.map((tick) => {
              const y = padTop + chartH - (tick / max) * chartH;
              return (
                <g key={tick}>
                  <line
                    x1={padX}
                    x2={width - padRight}
                    y1={y}
                    y2={y}
                    className={tick === 0 ? "tracker-chart-base" : "tracker-chart-grid"}
                  />
                  <text
                    x={padX - 10}
                    y={y + 4}
                    textAnchor="end"
                    className="tracker-axis-cap"
                  >
                    {tick}
                  </text>
                </g>
              );
            })}
            {isBar
              ? coords.map((p, i) => {
                  const h = Math.max(p.value > 0 ? 8 : 0, baseline - p.y);
                  return (
                    <g key={`${p.label}-${i}`} className="tracker-bar-group">
                      <title>{p.title || `${p.label}: ${p.value}`}</title>
                      <rect
                        x={p.x - barW / 2}
                        y={p.y}
                        width={barW}
                        height={h}
                        rx={4}
                        className={p.active ? "tracker-bar is-active" : "tracker-bar"}
                      />
                      {p.value > 0 && (!crowded || p.active) ? (
                        <text
                          x={p.x}
                          y={p.y - 10}
                          textAnchor="middle"
                          className="tracker-bar-val"
                        >
                          {p.value}
                        </text>
                      ) : null}
                      {labelAt(i, p.label) || points.length <= 8 ? (
                        <text
                          x={p.x}
                          y={baseline + 22}
                          textAnchor="middle"
                          className={p.active ? "tracker-bar-label is-active" : "tracker-bar-label"}
                        >
                          {p.label}
                        </text>
                      ) : null}
                    </g>
                  );
                })
              : (
                <>
                  {areaD ? <path d={areaD} className="tracker-area-fill" /> : null}
                  {lineD ? <path d={lineD} className="tracker-area-line" /> : null}
                  {coords.map((p, i) => (
                    <g key={`${p.label}-${i}`} className="tracker-area-point">
                      <title>{p.title || `${p.label}: ${p.value}`}</title>
                      <circle
                        cx={p.x}
                        cy={p.y}
                        r={p.active ? 7 : 5.5}
                        className={p.active ? "tracker-area-dot is-active" : "tracker-area-dot"}
                      />
                      {p.value > 0 && (!dense || p.active || labelAt(i, p.label)) && (!crowded || p.active || labelAt(i, p.label)) ? (
                        <text
                          x={p.x}
                          y={p.y - 14}
                          textAnchor="middle"
                          className="tracker-bar-val"
                        >
                          {p.value}
                        </text>
                      ) : null}
                      {labelAt(i, p.label) ? (
                        <text
                          x={p.x}
                          y={baseline + 22}
                          textAnchor="middle"
                          className={p.active ? "tracker-bar-label is-active" : "tracker-bar-label"}
                        >
                          {p.label}
                        </text>
                      ) : null}
                    </g>
                  ))}
                </>
              )}
          </svg>
        </div>
      )}
    </ChartFrame>
  );
}

type PieSlice = { key: string; label: string; value: number; color: string };

export function TrackerPieChart({
  title,
  unit = "item",
  unitPlural,
  slices,
  emptyText = "No data yet.",
  className,
  framed = true,
}: {
  title: string;
  unit?: string;
  unitPlural?: string;
  slices: PieSlice[];
  emptyText?: string;
  className?: string;
  framed?: boolean;
}) {
  const total = slices.reduce((sum, s) => sum + s.value, 0);
  const unitLabel = total === 1 ? unit : unitPlural || `${unit}s`;

  const cx = 110;
  const cy = 110;
  const r = 88;
  const inner = 52;
  let angle = 0;
  const arcs = slices
    .filter((s) => s.value > 0)
    .map((s) => {
      const sweep = total > 0 ? (s.value / total) * 360 : 0;
      const start = angle;
      const end = angle + sweep;
      angle = end;
      return { ...s, start, end };
    });

  const aria = slices.map((s) => `${s.label} ${s.value}`).join(", ");

  const body =
    total === 0 ? (
      <p className={emptyText === "—" ? "tracker-chart-dash" : "tracker-empty"}>{emptyText}</p>
    ) : (
      <div className="tracker-pie-layout">
        <svg
          className="tracker-pie-svg"
          viewBox="0 0 220 220"
          role="img"
          aria-label={aria}
        >
          {arcs.length === 1 ? (
            <>
              <circle cx={cx} cy={cy} r={r} fill={arcs[0].color} />
              <circle cx={cx} cy={cy} r={inner} fill="#fff" />
            </>
          ) : (
            arcs.map((s) => (
              <path key={s.key} d={donutSlice(cx, cy, r, inner, s.start, s.end)} fill={s.color}>
                <title>
                  {s.label}: {s.value}
                </title>
              </path>
            ))
          )}
          <text x={cx} y={cy - 6} textAnchor="middle" className="tracker-pie-total">
            {total}
          </text>
          <text x={cx} y={cy + 16} textAnchor="middle" className="tracker-pie-total-label">
            {unitLabel}
          </text>
        </svg>
        <ul className="tracker-pie-legend">
          {slices.map((s) => {
            const pct = total > 0 ? Math.round((s.value / total) * 100) : 0;
            return (
              <li key={s.key} className="tracker-pie-legend-row">
                <span className="tracker-pie-swatch" style={{ background: s.color }} />
                <span className="tracker-pie-legend-label">{s.label}</span>
                <span className="tracker-pie-legend-val">
                  {s.value}
                  <span className="tracker-pie-legend-pct">{pct}%</span>
                </span>
              </li>
            );
          })}
        </ul>
      </div>
    );

  if (!framed) {
    return <div className={className}>{body}</div>;
  }

  return (
    <ChartFrame
      className={className}
      title={title}
      subtitle={`${total} ${unitLabel}`}
    >
      {body}
    </ChartFrame>
  );
}

export function OutcomePieChart({
  pending = 0,
  rejected = 0,
  offered = 0,
  emptyText = "No interview outcomes yet.",
  className,
}: {
  pending?: number;
  rejected?: number;
  offered?: number;
  emptyText?: string;
  className?: string;
}) {
  return (
    <TrackerPieChart
      className={className}
      title="Interview outcomes"
      unit="company"
      unitPlural="companies"
      emptyText={emptyText}
      slices={[
        { key: "pending", label: "Pending", value: Math.max(0, pending), color: "#c9892a" },
        { key: "rejected", label: "Rejected", value: Math.max(0, rejected), color: "#c5221f" },
        { key: "offered", label: "Offered", value: Math.max(0, offered), color: "#1e7a4a" },
      ]}
    />
  );
}

export function ApplyTallyPad({
  value,
  onChange,
  onSave,
  saving,
  message,
}: {
  value: string;
  onChange: (next: string) => void;
  onSave: () => void;
  saving: boolean;
  message: string;
}) {
  const count = /^\d+$/.test(value.trim()) ? Number.parseInt(value.trim(), 10) : 0;
  const bump = (delta: number) => {
    const next = Math.max(0, Math.min(999, count + delta));
    onChange(String(next));
  };
  return (
    <form
      className="tracker-apply-today"
      onSubmit={(e) => {
        e.preventDefault();
        onSave();
      }}
    >
      <label className="tracker-apply-today-label" htmlFor="tracker-today-applies">
        Today&apos;s applies
      </label>
      <div className="tracker-apply-today-row">
        <input
          id="tracker-today-applies"
          type="number"
          min={0}
          max={999}
          step={1}
          inputMode="numeric"
          placeholder="0"
          className="tracker-apply-today-input"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          aria-label="Number of applications submitted today"
        />
        <button type="button" className="tracker-tally-step" onClick={() => bump(-1)} disabled={count === 0}>
          −
        </button>
        <button type="button" className="tracker-tally-step" onClick={() => bump(1)}>
          +
        </button>
        <button type="submit" className="tracker-btn tracker-apply-today-save" disabled={saving}>
          {saving ? "Saving…" : "Save"}
        </button>
      </div>
      {message ? (
        <p className={`tracker-apply-today-msg${/saved|stamp/i.test(message) ? " is-ok" : " is-error"}`}>
          {message}
        </p>
      ) : null}
    </form>
  );
}

function polar(cx: number, cy: number, r: number, angle: number): [number, number] {
  const a = ((angle - 90) * Math.PI) / 180;
  return [cx + r * Math.cos(a), cy + r * Math.sin(a)];
}

function donutSlice(
  cx: number,
  cy: number,
  outer: number,
  inner: number,
  start: number,
  end: number
): string {
  const [ox1, oy1] = polar(cx, cy, outer, start);
  const [ox2, oy2] = polar(cx, cy, outer, end);
  const [ix1, iy1] = polar(cx, cy, inner, end);
  const [ix2, iy2] = polar(cx, cy, inner, start);
  const large = end - start > 180 ? 1 : 0;
  return [
    `M ${ox1} ${oy1}`,
    `A ${outer} ${outer} 0 ${large} 1 ${ox2} ${oy2}`,
    `L ${ix1} ${iy1}`,
    `A ${inner} ${inner} 0 ${large} 0 ${ix2} ${iy2}`,
    "Z",
  ].join(" ");
}

export function TrackerMetric({
  label,
  value,
  note,
  small = false,
}: {
  label: string;
  value: ReactNode;
  note?: string;
  small?: boolean;
}) {
  return (
    <article className="tracker-metric">
      <p className="tracker-metric-label">{label}</p>
      <p className={`tracker-metric-value${small ? " tracker-metric-value-sm" : ""}`}>{value}</p>
      {note ? <p className="tracker-metric-note">{note}</p> : null}
    </article>
  );
}
