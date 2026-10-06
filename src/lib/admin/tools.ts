/** Tools shown on the /admin home grid. Superadmin-only surfaces live here. */
export type AdminTool = {
  id: string;
  title: string;
  blurb: string;
  cta: string;
  href: string;
  /** Not built yet — the tile renders as a disabled "Coming soon" card. */
  comingSoon?: boolean;
};

export const ADMIN_TOOLS: AdminTool[] = [
  {
    id: "resume-automation",
    title: "Resume Creation",
    blurb: "Generate a tailored resume for a candidate from their profile and a job description",
    cta: "Open automation",
    href: "/admin/resume-automation",
  },
];
