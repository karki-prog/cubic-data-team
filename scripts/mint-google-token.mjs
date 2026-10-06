// Re-mints the Gmail + Calendar/Drive refresh tokens the backend uses for
// user-scoped Google calls. Run when /api/jobs/google-health reports
// "invalid_grant" — a revoked refresh token can only be replaced by re-consent.
//
//   node scripts/mint-google-token.mjs
//
// Writes gmail-oauth.json and .calendar_oauth_token.json. Sign in as the
// mailbox/calendar owner (karki@cubicit.net), not your own account.
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const site = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const gmailFile = path.join(site, "gmail-oauth.json");
const calendarFile = path.join(site, ".calendar_oauth_token.json");

const SCOPES = [
  "openid",
  "https://www.googleapis.com/auth/userinfo.email",
  "https://www.googleapis.com/auth/gmail.send",
  "https://www.googleapis.com/auth/calendar",
  "https://www.googleapis.com/auth/drive.file",
  // Otter class attendance: read Meet participants of meetings karki@ organizes.
  "https://www.googleapis.com/auth/meetings.space.readonly",
];

const existing = JSON.parse(fs.readFileSync(gmailFile, "utf8"));
const clientId = process.env.GMAIL_CLIENT_ID || existing.client_id;
const clientSecret = process.env.GMAIL_CLIENT_SECRET || existing.client_secret;
if (!clientId || !clientSecret) {
  console.error("No desktop OAuth client found in gmail-oauth.json.");
  process.exit(1);
}

const server = http.createServer();
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const redirectUri = `http://127.0.0.1:${server.address().port}`;

const authUrl = `https://accounts.google.com/o/oauth2/v2/auth?${new URLSearchParams({
  client_id: clientId,
  redirect_uri: redirectUri,
  response_type: "code",
  scope: SCOPES.join(" "),
  access_type: "offline",
  // Without this Google returns no refresh_token for an account that already
  // consented, which is the whole point of running this.
  prompt: "consent",
})}`;

console.log(`[mint] open this and approve as the mailbox owner:\n${authUrl}\n`);

const code = await new Promise((resolve, reject) => {
  const timer = setTimeout(() => reject(new Error("timed out waiting for consent")), 5 * 60_000);
  server.on("request", (req, res) => {
    const url = new URL(req.url, redirectUri);
    const err = url.searchParams.get("error");
    const got = url.searchParams.get("code");
    res.writeHead(200, { "content-type": "text/plain" });
    res.end(err ? `Consent failed: ${err}` : "Done — you can close this tab.");
    clearTimeout(timer);
    if (err) reject(new Error(err));
    else if (got) resolve(got);
  });
});

const res = await fetch("https://oauth2.googleapis.com/token", {
  method: "POST",
  headers: { "content-type": "application/x-www-form-urlencoded" },
  body: new URLSearchParams({
    client_id: clientId,
    client_secret: clientSecret,
    code,
    grant_type: "authorization_code",
    redirect_uri: redirectUri,
  }),
});
const token = await res.json();
server.close();

if (!res.ok || !token.refresh_token) {
  console.error(`[mint] token exchange failed: ${JSON.stringify(token)}`);
  process.exit(1);
}

fs.writeFileSync(
  gmailFile,
  `${JSON.stringify(
    {
      client_id: clientId,
      client_secret: clientSecret,
      refresh_token: token.refresh_token,
      token_uri: "https://oauth2.googleapis.com/token",
      type: "authorized_user",
    },
    null,
    2
  )}\n`
);
fs.writeFileSync(calendarFile, `${JSON.stringify(token, null, 2)}\n`);

const who = token.id_token
  ? JSON.parse(Buffer.from(token.id_token.split(".")[1], "base64url").toString()).email
  : "unknown";
console.log(`[mint] wrote gmail-oauth.json + .calendar_oauth_token.json for ${who}`);
console.log("[mint] restart the backend, then re-run /api/jobs/google-health");
