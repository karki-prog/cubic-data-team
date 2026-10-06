import type { Metadata } from "next";
import { CubicInterviews } from "./CubicInterviews";

export const metadata: Metadata = {
  title: "Cubic Interviews — Cubic Data Dashboard",
  description: "Companies that gave Cubic candidates interviews",
};

export default function InterviewsPage() {
  return <CubicInterviews />;
}
