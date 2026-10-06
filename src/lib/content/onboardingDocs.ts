/** Onboarding documents from Downloads/Data Documents. */
export type OnboardingDoc = {
  id: string;
  title: string;
  description?: string;
  /** File type label, e.g. PDF, DOCX */
  type: string;
  /**
   * Download URL under `public/onboarding/`.
   */
  href: string;
  /** If true, opens in a new tab instead of forcing download */
  openInNewTab?: boolean;
};

/** Files copied from `~/Downloads/Data Documents`. */
export const ONBOARDING_DOCS: OnboardingDoc[] = [
  {
    id: "introduction-for-data-engineer",
    title: "Introduction for Data Engineer",
    type: "DOCX",
    href: "/onboarding/introduction-for-data-engineer.docx",
  },
  {
    id: "data-engineering-job-titles",
    title: "Data Engineering Job Titles",
    type: "DOCX",
    href: "/onboarding/data-engineering-job-titles.docx",
  },
  {
    id: "data-engineer-basic-interview-questions",
    title: "Data Engineer Basic Interview Questions",
    type: "DOCX",
    href: "/onboarding/data-engineer-basic-interview-questions.docx",
  },
  {
    id: "data-project-answer",
    title: "Data Project Answer",
    type: "DOCX",
    href: "/onboarding/data-project-answer.docx",
  },
  {
    id: "data-related-job-search-steps-for-linkedin",
    title: "Data Job Search Steps for LinkedIn",
    type: "DOCX",
    href: "/onboarding/data-related-job-seach-steps-for-linkedin.docx",
  },
  {
    id: "full-technical-terms-list",
    title: "Full Technical Terms List",
    type: "DOCX",
    href: "/onboarding/full-technical-terms-list.docx",
  },
  {
    id: "tech-stack-tools-and-technical-terms",
    title: "Tech Stack, Tools & Technical Terms",
    description: "Senior Data Engineer",
    type: "DOCX",
    href: "/onboarding/tech-stack-tools-and-technical-terms-for-a-senior-data-engineer.docx",
  },
  {
    id: "google-voice-steps",
    title: "Google Voice Steps",
    type: "DOCX",
    href: "/onboarding/google-voice-steps.docx",
  },
  {
    id: "interview-email-threads",
    title: "Interview Email Threads",
    type: "DOCX",
    href: "/onboarding/interview-email-threads.docx",
  },
  {
    id: "linkedin-recruiter-reachout-reply",
    title: "LinkedIn Recruiter Reach-Out & Reply",
    type: "DOCX",
    href: "/onboarding/linkedin-recruiter-reachout-reply.docx",
  },
  {
    id: "recruiter-call-basic-question",
    title: "Recruiter Call Basic Questions",
    type: "DOCX",
    href: "/onboarding/recruiter-call-basic-question.docx",
  },
  {
    id: "prompt-to-take-vendor-call-for-chatgpt",
    title: "Vendor Call Prompt (ChatGPT)",
    type: "DOCX",
    href: "/onboarding/prompt-to-take-vendor-call-for-chatgpt.docx",
  },
  {
    id: "sample-interview-tracking-sheet",
    title: "Sample Interview Tracking Sheet",
    type: "XLSX",
    href: "/onboarding/sample-interview-tracking-sheet.xlsx",
  },
];
