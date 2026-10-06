/** Tools shown on the Cubic Data Team home grid. */
export type TeamTool = {
  id: string;
  title: string;
  blurb: string;
  /** Tile CTA label (links / actions). */
  cta?: string;
  /** If set, card opens this URL in a new tab. */
  url?: string;
  /** Opens a dashboard modal. */
  action?: "onboarding" | "practice-classes" | "do-not-apply" | "resume-prompt";
  /** Stays usable on phones. Everything else is desktop-only (the booking /
   *  tracker / hiring UIs need a wide screen). */
  mobileOk?: boolean;
};

export const TEAM_TOOLS: TeamTool[] = [
  {
    id: "daily-apply-list",
    title: "Job Hiring",
    blurb: "Current openings searched by the Cubic team",
    cta: "View hiring",
    url: "/hiring",
  },
  {
    id: "cubic-interviews",
    title: "Cubic Interviews",
    blurb: "Interviews received by Cubic in the last 15 days and 5 days ahead",
    cta: "View interviews",
    url: "/interviews",
    mobileOk: true,
  },
  {
    id: "do-not-apply",
    title: "Do Not Apply List",
    blurb: "Companies you should skip applying to",
    cta: "View companies",
    action: "do-not-apply",
    mobileOk: true,
  },
  {
    id: "interview-booking",
    title: "Interview Booking",
    blurb: "Live interview slot availability",
    cta: "Open Booking",
    url: "/book/appointment",
  },
  {
    id: "phone-call-booking",
    title: "Phone Call Slot Booking",
    blurb: "Support for phone call booking",
    cta: "Open Booking",
    url: "/book/phone-call",
  },
    {
      id: "application-tracker",
      title: "Application Tracker",
      blurb: "Your applies, interviews, resumes, and JDs in one place",
      cta: "Open Tracker",
      url: "/tracker",
      mobileOk: true,
    },
  {
    id: "resume-retargeting-prompt",
    title: "Cover Letter Tailoring Prompt",
    blurb: "Prompt that writes a cover letter tailored to the job",
    cta: "View prompt",
    action: "resume-prompt",
  },
  {
    id: "assessment-booking",
    title: "Assessment Booking",
    blurb: "Schedule candidate assessments",
  },
  {
    id: "onboarding-list",
    title: "Onboarding List",
    blurb: "Download Data Documents pack",
    cta: "View documents",
    action: "onboarding",
    mobileOk: true,
  },
  {
    id: "practice-classes",
    title: "Otter & Pronunciation",
    blurb: "Mon–Fri 11:30 AM – 12:30 PM & 2:00 PM – 3:00 PM CST — one class, one join link",
    cta: "Join class & attendance",
    action: "practice-classes",
    mobileOk: true,
  },
  {
    id: "support-feedback",
    title: "Support Feedback Form",
    blurb: "Anonymous feedback on interview support and suggestions",
    cta: "Open form",
    url: "https://docs.google.com/forms/d/1vi9Nmivw6olLz_WAMUoTvP2ZOTCp1UxI44rZqR5ZpYw/viewform",
    mobileOk: true,
  },
];
