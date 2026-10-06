import type { Metadata } from "next";
import { PhoneCallAvailability } from "./PhoneCallAvailability";

export const metadata: Metadata = {
  title: "Phone Call Slot Booking — Cubic Data Team",
  description: "Support for phone call booking",
};

export default function PhoneCallBookingPage() {
  return <PhoneCallAvailability />;
}
