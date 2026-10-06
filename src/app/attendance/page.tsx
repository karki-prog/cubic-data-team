import type { Metadata } from "next";
import { OtterAttendance } from "./OtterAttendance";

export const metadata: Metadata = {
  title: "Otter Attendance — Cubic Data Dashboard",
  description: "Otter & Pronunciation class attendance and time in class",
};

export default function AttendancePage() {
  return <OtterAttendance />;
}
