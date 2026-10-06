import type { Metadata } from "next";
import { AppointmentAvailability } from "./AppointmentAvailability";

export const metadata: Metadata = {
  title: "Appointment Availability — Cubic Data Dashboard",
  description: "Live interview slots from the Cubic Interview Sheet.",
};

export default function AppointmentBookingPage() {
  return <AppointmentAvailability />;
}
