import fs from "fs";
import type { NextConfig } from "next";
import path from "path";

const isDev = process.env.NODE_ENV !== "production";
const appVersion = isDev
  ? "dev"
  : (process.env.CUBIC_APP_VERSION?.trim() || `b${Date.now()}`);

if (!isDev) {
  fs.writeFileSync(
    path.join(__dirname, "public", "version.json"),
    `${JSON.stringify({ version: appVersion })}\n`,
  );
}

const nextConfig: NextConfig = {
  output: "export",
  images: { unoptimized: true },
  env: {
    NEXT_PUBLIC_APP_VERSION: appVersion,
  },
  generateBuildId: async () => appVersion,
  // Static export wants /login/; next dev must not rewrite /api/auth/google → /api/auth/google/.
  trailingSlash: !isDev,
  eslint: {
    ignoreDuringBuilds: true,
  },
  allowedDevOrigins: ["http://localhost:3000", "http://127.0.0.1:3000"],
  turbopack: {
    root: path.join(__dirname),
  },
};

export default nextConfig;
