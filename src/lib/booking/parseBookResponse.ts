export function bookSubmitErrorMessage(err: unknown): string {
  if (err instanceof TypeError) {
    const message = err.message || "";
    if (/network connection was lost|Failed to fetch|Load failed|NetworkError/i.test(message)) {
      return "Upload failed. Check your connection and try a smaller PDF.";
    }
  }
  const raw = err instanceof Error ? err.message : String(err);
  return sanitizeBookError(raw);
}

export function sanitizeBookError(raw: string): string {
  const text = String(raw || "").trim();
  if (!text) return "Submission failed";
  if (
    /RATE_LIMIT_EXCEEDED|RESOURCE_EXHAUSTED|Quota exceeded|Read requests per minute|sheets\.googleapis\.com/i.test(
      text
    )
  ) {
    return "Google Sheets is busy right now. Wait about a minute and try again.";
  }
  if (/^\s*\{[\s\S]*"error"[\s\S]*\}\s*$/.test(text) || text.includes('"error": {')) {
    return "Google is temporarily unavailable. Try again in a moment.";
  }
  if (text.length > 220) {
    return "Something went wrong. Try again in a moment.";
  }
  return text;
}

export async function parseBookResponse(res: Response) {
  const text = await res.text();
  let json: { ok?: boolean; error?: string; result?: { folderPath?: string } } = {};
  try {
    json = JSON.parse(text) as typeof json;
  } catch {
    json = {};
  }

  if (res.status === 401) {
    throw new Error("Please sign in again, then submit.");
  }
  if (res.status === 409) {
    throw new Error(sanitizeBookError(json.error || "This time was just taken. Pick another slot."));
  }
  if (res.status === 413) {
    throw new Error("Resume file is too large. Use a PDF under 5 MB.");
  }
  if (res.status === 429) {
    throw new Error("Google Sheets is busy right now. Wait about a minute and try again.");
  }
  if (res.status === 504 || res.status === 502) {
    throw new Error(sanitizeBookError(json.error || "Submission timed out. Try again in a moment."));
  }
  if (!res.ok || !json.ok) {
    throw new Error(sanitizeBookError(json.error || "Submission failed"));
  }
  return json;
}
