export type HiringJob = {
  title: string;
  displayTitle?: string;
  displaySubtitle?: string | null;
  url: string;
  addedBy: string;
  company: string;
  jobTitle: string;
  /** Which ATS / job board the posting lives on — derived from the URL. */
  jobSite: string;
  location: string;
  salary: string;
  workMode: string;
};

export type HiringDayGroup = {
  label: string;
  jobs: HiringJob[];
};
