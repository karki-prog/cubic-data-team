export type PhoneBlock = {
  start: number;
  end: number;
  type: "open" | "partial" | "full" | "emergency";
  minFree: number;
  capacity: number;
  inputTime?: string;
  label?: string;
};

export type PhoneDay = {
  label: string;
  dateIso: string;
  blocks: PhoneBlock[];
};
