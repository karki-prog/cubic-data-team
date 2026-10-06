/** Shared Meet link + class schedule. */

export const PRACTICE_MEET_URL = "https://meet.google.com/qvf-yine-evq";

export type PracticeSession = {
  label: string;
  time: string;
};

export type PracticeClassGroup = {
  id: string;
  title: string;
  tagline: string;
  sessions: PracticeSession[];
  joinUrl: string;
};

/** Single combined Otter & Pronunciation class. */
export const PRACTICE_CLASSES: PracticeClassGroup[] = [
  {
    id: "otter-pronunciation",
    title: "Otter & Pronunciation Class",
    tagline:
      "Live speaking practice — right pronunciation, clearer speech, and interview-ready delivery",
    sessions: [
      { label: "Monday – Friday", time: "11:30 AM – 12:30 PM CST" },
      { label: "Monday – Friday", time: "2:00 PM – 3:00 PM CST" },
    ],
    joinUrl: PRACTICE_MEET_URL,
  },
];
