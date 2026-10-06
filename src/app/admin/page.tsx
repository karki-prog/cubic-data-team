import type { Metadata } from "next";
import { AdminHome } from "./AdminHome";

export const metadata: Metadata = {
  title: "Admin — Cubic Data Dashboard",
  description: "Staff-only tools for the Cubic Data team",
};

export default function AdminPage() {
  return <AdminHome />;
}
