/** One row of the "Client List - Interview recieved" tab, as served by /api/cubic-interviews. */
export type InterviewClient = {
  company: string;
  total: number;
  data: number;
  java: number;
  visas: { label: string; count: number }[];
  interviewDates: string[];
  url: string;
  linkLabel: string;
  comment: string;
};

export type CubicInterviewsResponse = {
  ok: boolean;
  error?: string;
  generatedAt?: string;
  windowNote?: string;
  count?: number;
  totals?: { interviews: number; data: number; java: number };
  signature?: string;
  clients?: InterviewClient[];
};
