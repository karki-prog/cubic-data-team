import type { Metadata } from "next";
import { ResumeAutomation } from "./ResumeAutomation";

export const metadata: Metadata = {
  title: "Resume Creation — Cubic Admin",
  description: "Generate a tailored resume for a candidate",
};

export default function ResumeAutomationPage() {
  return <ResumeAutomation />;
}
