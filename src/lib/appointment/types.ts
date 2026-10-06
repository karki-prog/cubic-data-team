export type AppointmentBlock = {
  start: number;
  end: number;
  type: "open" | "partial" | "full" | "emergency";
  minFree: number;
  capacity: number;
  inputTime?: string;
  label?: string;
};

export type AppointmentDay = {
  label: string;
  dateIso: string;
  blocks: AppointmentBlock[];
};
