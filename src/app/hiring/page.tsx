import type { Metadata } from "next";
import { HiringPostings } from "./HiringPostings";

export const metadata: Metadata = {
  title: "Job Hiring — Cubic Data Dashboard",
  description: "Current openings searched by the Cubic team",
};

export default function HiringPage() {
  return <HiringPostings />;
}
