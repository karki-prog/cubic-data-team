/** Display helpers for minutes-from-midnight. Slot math lives in the Rust API. */

export type UsZone = "EST" | "CST" | "MST" | "PST";

export const US_ZONES: UsZone[] = ["EST", "CST", "MST", "PST"];

/** Minutes to add to a CST clock to get the same instant in that zone. */
export const ZONE_OFFSET_FROM_CST: Record<UsZone, number> = {
  EST: 60,
  CST: 0,
  MST: -60,
  PST: -120,
};

export function parseUsZone(raw: string | undefined | null): UsZone {
  const z = String(raw || "").trim().toUpperCase();
  if (z === "EST" || z === "EDT") return "EST";
  if (z === "MST" || z === "MDT") return "MST";
  if (z === "PST" || z === "PDT") return "PST";
  return "CST";
}

export function wrapMins(totalMins: number): number {
  return ((totalMins % (24 * 60)) + 24 * 60) % (24 * 60);
}

export function cstToZone(cstMin: number, zone: UsZone): number {
  return wrapMins(cstMin + ZONE_OFFSET_FROM_CST[zone]);
}

export function zoneToCst(zoneMin: number, zone: UsZone): number {
  return wrapMins(zoneMin - ZONE_OFFSET_FROM_CST[zone]);
}

export function minsToTimeStr(totalMins: number): string {
  const wrapped = wrapMins(totalMins);
  let h = Math.floor(wrapped / 60);
  const m = wrapped % 60;
  const ampm = h >= 12 ? "PM" : "AM";
  h = h % 12;
  if (h === 0) h = 12;
  return m === 0 ? `${h} ${ampm}` : `${h}:${String(m).padStart(2, "0")} ${ampm}`;
}

export function minsToInputTime(totalMins: number): string {
  const wrapped = wrapMins(totalMins);
  const h = Math.floor(wrapped / 60);
  const m = wrapped % 60;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
}

/** Slot times are stored in America/Chicago. */
export function minsToCstLabel(totalMins: number): string {
  return `${minsToTimeStr(totalMins)} CST`;
}

export function meetingTimeForSubmit(
  hhmmOrLabel: string,
  startMin: number | null,
  emergency: boolean,
  zone: UsZone = "CST"
): string {
  const raw = hhmmOrLabel.trim();
  if (raw) {
    const fromLabel = raw.match(/\b(EST|EDT|CST|CDT|MST|MDT|PST|PDT)\b/i)?.[1];
    const sourceZone = parseUsZone(fromLabel || zone);
    const match = raw.match(/^(\d{1,2}):(\d{2})(?::\d{2}(?:\.\d+)?)?/);
    if (match) {
      return minsToCstLabel(zoneToCst(Number(match[1]) * 60 + Number(match[2]), sourceZone));
    }
    const mins = inputTimeToMinutes(raw);
    if (mins != null) {
      return minsToCstLabel(zoneToCst(mins, sourceZone));
    }
    if (/\bCST\b|\bCDT\b/i.test(raw)) {
      return raw.replace(/\bCDT\b/i, "CST").replace(/\s+/g, " ").trim();
    }
    return `${raw} CST`;
  }
  if (startMin != null) {
    return minsToCstLabel(startMin);
  }
  return "";
}

/** Native `<input type="time">` value → minutes from midnight. */
export function inputTimeToMinutes(hhmm: string): number | null {
  const match = hhmm.trim().match(/^(\d{1,2}):(\d{2})(?::\d{2}(?:\.\d+)?)?$/);
  if (!match) return null;
  const h = Number(match[1]);
  const m = Number(match[2]);
  if (h > 23 || m > 59) return null;
  return h * 60 + m;
}

export function durationToMinutes(label: string): number {
  const text = label.trim().toLowerCase();
  if (!text) return 0;
  if (text.includes("4 hr +")) return 240;
  const hr = text.match(/(\d+)\s*hr/);
  const min = text.match(/(\d+)\s*min/);
  const total = (hr ? Number(hr[1]) * 60 : 0) + (min ? Number(min[1]) : 0);
  return total > 0 ? total : 0;
}
