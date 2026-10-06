import type { Metadata } from "next";
import { ApplicationTracker } from "./ApplicationTracker";

export const metadata: Metadata = {
  title: "Application Tracker — Cubic Data Dashboard",
  description: "Follow up on your applies, interviews, resumes, and job descriptions",
};

export default function TrackerPage() {
  return <ApplicationTracker />;
}
