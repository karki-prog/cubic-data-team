/** Shapes served by /api/otter-attendance. */
/** One stretch in the call: joined at `from`, left at `to` (Chicago time). */
export type AttendanceJoin = { from: string; to: string; minutes: number };

export type AttendanceSession = { session: string; minutes: number; joins?: AttendanceJoin[] };

export type AttendanceDay = {
  present: boolean;
  minutes: number;
  sessions: AttendanceSession[];
};

export type AttendanceCandidate = {
  name: string;
  email: string;
  row: number;
  presentDays: number;
  totalMinutes: number;
  /** "Otter Rating" from the Data candidate sheet (Current_Market), when set. */
  grade?: string;
  /** Keyed by YYYY-MM-DD; only days with a tick or recorded minutes. */
  days: Record<string, AttendanceDay>;
};

export type AttendanceResponse = {
  ok: boolean;
  error?: string;
  isStaff?: boolean;
  month?: string;
  year?: number;
  months?: { key: string; label: string }[];
  today?: string;
  minMinutes?: number;
  days?: { key: string; day: number; dow: string }[];
  candidates?: AttendanceCandidate[];
};
