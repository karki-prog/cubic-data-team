"use client";

import { useEffect, useLayoutEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import { ResumeFileField, MAX_RESUME_BYTES } from "@/components/booking/ResumeFileField";
import {
  bookSubmitErrorMessage,
  parseBookResponse,
  sanitizeBookError,
} from "@/lib/booking/parseBookResponse";
import {
  cstToZone,
  durationToMinutes,
  inputTimeToMinutes,
  meetingTimeForSubmit,
  minsToInputTime,
  minsToTimeStr,
  parseUsZone,
  US_ZONES,
  zoneToCst,
  type UsZone,
} from "@/lib/ui/clock";
import { useSlotHold } from "@/lib/booking/useSlotHold";
import { StatusToast, useStatusToast } from "@/components/ui/StatusToast";

export type BookPrefill = {
  dateIso: string;
  startMin: number | null;
  meetingTime: string;
  meetingDuration: string;
  emergency: boolean;
  subtitle: string;
  timeZone?: string;
  /** Hold reserved by the availability grid before this modal opened. */
  holdId?: string;
  holdExpiresAt?: string;
};

type Props = {
  open: boolean;
  prefill: BookPrefill | null;
  onClose: () => void;
  onSuccess: () => void;
};

const MODES = ["Zoom", "Google Meet", "Microsoft Teams", "Other"] as const;
const LOCATIONS = ["Onsite", "Remote", "Hybrid"] as const;
const DURATIONS = [
  "15 min",
  "30 min",
  "45 min",
  "1 Hr",
  "1 Hr 30 min",
  "2 Hr",
  "3 Hr",
  "4 Hr",
  "4 Hr +",
] as const;

function prettyDate(dateIso: string) {
  const [y, m, d] = dateIso.split("-").map(Number);
  if (!y || !m || !d) return dateIso;
  const months = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
  ];
  return `${months[m - 1]} ${d}, ${y}`;
}

const TIME_OPTIONS = Array.from({ length: 65 }, (_, i) => 360 + i * 15);

function normalize(s: string) {
  return s.toLowerCase().replace(/_/g, " ").replace(/\s+/g, " ").trim();
}

export function PhoneCallBookModal({ open, prefill, onClose, onSuccess }: Props) {
  const [candidates, setCandidates] = useState<string[]>([]);
  const [query, setQuery] = useState("");
  const [candidateName, setCandidateName] = useState("");
  const [showSuggestions, setShowSuggestions] = useState(false);
  const [showTimePicker, setShowTimePicker] = useState(false);
  const [mode, setMode] = useState("");
  const [location, setLocation] = useState("");
  const [meetingDate, setMeetingDate] = useState("");
  const [meetingTime, setMeetingTime] = useState("");
  const [startMin, setStartMin] = useState<number | null>(null);
  const [timeZone, setTimeZone] = useState<UsZone>("CST");
  const [meetingDuration, setMeetingDuration] = useState("30 min");
  const [client, setClient] = useState("");
  const [vendor, setVendor] = useState("");
  const [panel, setPanel] = useState("");
  const [specialNote, setSpecialNote] = useState("");
  const [jobDescription, setJobDescription] = useState("");
  const [resumeFile, setResumeFile] = useState<File | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");
  const [done, setDone] = useState(false);
  const [folderPath, setFolderPath] = useState("");
  const [formReady, setFormReady] = useState(false);
  const [animIn, setAnimIn] = useState(false);
  const searchWrapRef = useRef<HTMLDivElement>(null);
  const suggestionsRef = useRef<HTMLUListElement>(null);
  const timePickerRef = useRef<HTMLDivElement>(null);
  const openTokenRef = useRef(0);

  useEffect(() => {
    if (!open) {
      setAnimIn(false);
      setFormReady(false);
      return;
    }
    const html = document.documentElement;
    const body = document.body;
    const prevHtmlOverflow = html.style.overflow;
    const prevBodyOverflow = body.style.overflow;
    const prevBodyPaddingRight = body.style.paddingRight;
    const scrollbarGap = window.innerWidth - html.clientWidth;
    html.style.overflow = "hidden";
    body.style.overflow = "hidden";
    if (scrollbarGap > 0) {
      body.style.paddingRight = `${scrollbarGap}px`;
    }
    // Paint overlay first, then kick animation on next frame (matches Apps Script).
    const raf = requestAnimationFrame(() => setAnimIn(true));
    return () => {
      cancelAnimationFrame(raf);
      html.style.overflow = prevHtmlOverflow;
      body.style.overflow = prevBodyOverflow;
      body.style.paddingRight = prevBodyPaddingRight;
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    setError("");
    setDone(false);
    setFolderPath("");
    setFormReady(false);
    setQuery("");
    setCandidateName("");
    setMode("");
    setLocation("");
    setClient("");
    setVendor("");
    setPanel("");
    setSpecialNote("");
    setJobDescription("");
    setResumeFile(null);
    setMeetingDate(prefill?.dateIso || "");
    const zone = parseUsZone(prefill?.timeZone);
    setTimeZone(zone);
    setMeetingTime(
      prefill?.startMin != null
        ? minsToInputTime(cstToZone(prefill.startMin, zone))
        : ""
    );
    setStartMin(prefill?.startMin ?? null);
    setMeetingDuration(prefill?.meetingDuration || "30 min");
  }, [open, prefill]);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    const token = ++openTokenRef.current;
    setFormReady(false);

    const minDelay = new Promise((r) => setTimeout(r, 450));
    const load = fetch("/api/phone-call-candidates", { cache: "no-store" })
      .then((r) => r.json())
      .then((json) => {
        if (cancelled || token !== openTokenRef.current) return;
        if (json?.ok && Array.isArray(json.candidates)) {
          setCandidates(json.candidates);
        }
      })
      .catch(() => {
        if (!cancelled && token === openTokenRef.current) {
          setError("Could not load candidates. Refresh and try again.");
        }
      });

    Promise.all([load, minDelay]).finally(() => {
      if (cancelled || token !== openTokenRef.current) return;
      setFormReady(true);
    });

    return () => {
      cancelled = true;
    };
  }, [open]);

  useEffect(() => {
    function onDocClick(e: MouseEvent) {
      const target = e.target as Node;
      if (
        !searchWrapRef.current?.contains(target) &&
        !suggestionsRef.current?.contains(target)
      ) {
        setShowSuggestions(false);
      }
      if (!timePickerRef.current?.contains(e.target as Node)) {
        setShowTimePicker(false);
      }
    }
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  }, []);

  const filtered = useMemo(() => {
    const q = normalize(query);
    if (!q) return candidates.slice(0, 12);
    return candidates.filter((n) => normalize(n).includes(q)).slice(0, 12);
  }, [candidates, query]);
  const suggestionsOpen = showSuggestions && filtered.length > 0;

  useLayoutEffect(() => {
    if (!suggestionsOpen) return;
    const wrap = searchWrapRef.current;
    const list = suggestionsRef.current;
    if (!wrap || !list) return;
    const place = () => {
      const rect = wrap.getBoundingClientRect();
      list.style.left = `${rect.left}px`;
      list.style.top = `${rect.bottom + 4}px`;
      list.style.width = `${rect.width}px`;
    };
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => {
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [suggestionsOpen, filtered]);

  useEffect(() => {
    if (showTimePicker) {
      const activeEl = timePickerRef.current?.querySelector(".pca-time-popup-opt.active");
      if (activeEl) {
        activeEl.scrollIntoView({ block: "nearest" });
      }
    }
  }, [showTimePicker]);

  const hold = useSlotHold({
    open: open && !!prefill,
    kind: "phone",
    dateIso: prefill?.dateIso || "",
    startMin: startMin ?? prefill?.startMin ?? null,
    durationMin: durationToMinutes(meetingDuration) || 30,
    emergency: prefill?.emergency ?? false,
    providedHoldId: prefill?.holdId,
  });
  const status = useStatusToast();

  useEffect(() => {
    if (hold.holdError) status.show("error", sanitizeBookError(hold.holdError));
  }, [hold.holdError, status.show]);

  useEffect(() => {
    if (error) status.show("error", sanitizeBookError(error));
  }, [error, status.show]);

  useEffect(() => {
    if (done) status.show("success", "Phone call request submitted.");
  }, [done, status.show]);

  if (!open || !prefill) return null;

  const slot = prefill;
  const effectiveCstMin =
    startMin != null
      ? startMin
      : inputTimeToMinutes(meetingTime) != null
        ? zoneToCst(inputTimeToMinutes(meetingTime)!, timeZone)
        : null;

  const displaySubtitle =
    submitting
      ? "Please wait…"
      : done
        ? "Your phone call request was submitted"
        : meetingDate && effectiveCstMin != null
          ? slot.emergency
            ? `Emergency phone call for ${prettyDate(meetingDate)} · ${minsToTimeStr(cstToZone(effectiveCstMin, timeZone))} ${timeZone}`
            : `Phone call for ${prettyDate(meetingDate)} · ${minsToTimeStr(cstToZone(effectiveCstMin, timeZone))} ${timeZone}`
          : slot.subtitle;

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (submitting || done) return;
    setError("");

    if (!candidateName) {
      setError("Select a candidate from the search list.");
      return;
    }
    if (!mode) {
      setError("Mode of Interview is required.");
      return;
    }
    if (!location) {
      setError("Location is required.");
      return;
    }
    if (!meetingDate) {
      setError("Meeting Date is required.");
      return;
    }
    if (!meetingTime) {
      setError("Meeting Time is required.");
      return;
    }
    if (!meetingDuration) {
      setError("Meeting Duration is required.");
      return;
    }
    if (!client.trim()) {
      setError("Client is required.");
      return;
    }
    if (!panel.trim()) {
      setError("Panel is required.");
      return;
    }
    if (!jobDescription.trim()) {
      setError("Job Description is required.");
      return;
    }
    if (!resumeFile) {
      setError("Please choose a resume file.");
      return;
    }
    if (resumeFile.size > MAX_RESUME_BYTES) {
      setError("File too big. Use a resume under 5 MB.");
      setResumeFile(null);
      return;
    }

    if (!slot.emergency && !hold.holdId) {
      setError(hold.holdError || "This slot is no longer reserved. Close and pick it again.");
      return;
    }

    setSubmitting(true);
    try {
      let note = specialNote.trim();
      if (slot.emergency) {
        const tag = "EMERGENCY REQUEST";
        note = note ? `${tag} — ${note}` : tag;
      }

      const form = new FormData();
      form.append("candidateName", candidateName);
      form.append("location", location);
      form.append("interviewPlatform", mode);
      form.append("meetingDate", meetingDate);
      form.append(
        "meetingTime",
        meetingTimeForSubmit(meetingTime, effectiveCstMin, slot.emergency, timeZone)
      );
      form.append("timeZone", timeZone);
      form.append("meetingDuration", meetingDuration);
      form.append("panel", panel.trim());
      form.append("client", client.trim());
      form.append("vendor", vendor.trim());
      form.append("jobDescription", jobDescription.trim());
      form.append("specialNote", note);
      form.append("resume", resumeFile, resumeFile.name);
      form.append("emergency", slot.emergency ? "true" : "false");
      if (hold.holdId) {
        form.append("holdId", hold.holdId);
      }

      const res = await fetch("/api/phone-call-book", {
        method: "POST",
        credentials: "include",
        body: form,
      });
      const json = await parseBookResponse(res);
      setFolderPath(String(json.result?.folderPath || ""));
      setDone(true);
      onSuccess();
    } catch (err) {
      setError(bookSubmitErrorMessage(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div
      className={`pca-book-backdrop${animIn ? " show" : ""}`}
      role="presentation"
      onClick={onClose}
    >
      <div
        className={`pca-book-modal pca-book-modal-wide${done ? " mode-thanks" : ""}${submitting ? " mode-submitting" : ""}${animIn ? " show" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="pca-book-title"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="pca-book-head">
          <div className="pca-book-head-spacer" aria-hidden />
          <div className="pca-book-title-wrap">
            <h2 id="pca-book-title">
              {submitting
                ? "Submitting"
                : done
                  ? "Thank you"
                  : "Phone Call Request"}
            </h2>
            <p className="pca-book-sub">
              {displaySubtitle}
            </p>
          </div>
          <div className="pca-book-actions">
            <button
              type="button"
              className="pca-book-close"
              aria-label="Close"
              onClick={onClose}
              disabled={submitting}
            >
              ✕
            </button>
          </div>
        </div>

        {!done && !submitting && (!formReady || !hold.ready) ? (
          <div className="pca-book-loading" aria-live="polite" aria-busy="true">
            <div className="pca-book-spinner" aria-hidden />
            {formReady && !hold.ready ? "Reserving this slot…" : "Loading booking form…"}
          </div>
        ) : null}

        {done ? (
          <div className="pca-book-status">
            <div className="pca-book-thanks-icon" aria-hidden>
              ✓
            </div>
            <h3>Thank you</h3>
            <p>
              Your phone call request is in. Drive, the sheet, and email will
              finish in the background
              {folderPath ? (
                <>
                  {" "}
                  under <strong>{folderPath}</strong>
                </>
              ) : null}
              .
            </p>
            <button type="button" className="pca-book-submit" onClick={onClose}>
              Done
            </button>
          </div>
        ) : null}

        {submitting ? (
          <div className="pca-book-status" aria-live="polite" aria-busy="true">
            <div className="pca-book-spinner" aria-hidden />
            <h3>Saving your request</h3>
            <p>This should only take a moment…</p>
          </div>
        ) : null}

        {formReady && hold.ready && !done && !submitting ? (
          <form className="pca-book-form pca-book-form-in" onSubmit={handleSubmit} noValidate>
            <div className="pca-field">
              <label className="req" htmlFor="pca-candidate">
                Candidate Name
              </label>
              <div
                className={`pca-search-wrap${showSuggestions && filtered.length > 0 ? " open" : ""}`}
                ref={searchWrapRef}
              >
                <input
                  id="pca-candidate"
                  type="text"
                  autoComplete="off"
                  placeholder="Type to search candidates…"
                  value={query}
                  onChange={(e) => {
                    setQuery(e.target.value);
                    setCandidateName("");
                    setShowSuggestions(true);
                  }}
                  onFocus={() => setShowSuggestions(true)}
                  required
                  className={candidateName ? "pca-candidate-picked" : undefined}
                />
                {candidateName ? (
                  <span className="pca-picked-tick" aria-label="Candidate selected">
                    <svg viewBox="0 0 20 20" fill="none" aria-hidden>
                      <circle cx="10" cy="10" r="10" fill="#157f3d" />
                      <path
                        d="M6 10.2l2.4 2.4L14.2 7"
                        stroke="#fff"
                        strokeWidth="2"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      />
                    </svg>
                  </span>
                ) : null}
                {suggestionsOpen
                  ? createPortal(
                      <ul className="pca-suggestions" role="listbox" ref={suggestionsRef}>
                        {filtered.map((name) => (
                          <li key={name}>
                            <button
                              type="button"
                              onMouseDown={(e) => e.preventDefault()}
                              onClick={() => {
                                setCandidateName(name);
                                setQuery(name);
                                setShowSuggestions(false);
                              }}
                            >
                              {name}
                            </button>
                          </li>
                        ))}
                      </ul>,
                      document.body,
                    )
                  : null}
              </div>
            </div>

            <div className="pca-row pca-row-2">
              <div className="pca-field">
                <label className="req" htmlFor="pca-mode">
                  Mode of Interview
                </label>
                <select
                  id="pca-mode"
                  value={mode}
                  onChange={(e) => setMode(e.target.value)}
                  required
                >
                  <option value="">Select mode</option>
                  {MODES.map((m) => (
                    <option key={m} value={m}>
                      {m}
                    </option>
                  ))}
                </select>
              </div>
              <div className="pca-field">
                <label className="req" htmlFor="pca-location">
                  Location
                </label>
                <select
                  id="pca-location"
                  value={location}
                  onChange={(e) => setLocation(e.target.value)}
                  required
                >
                  <option value="">Select location</option>
                  {LOCATIONS.map((m) => (
                    <option key={m} value={m}>
                      {m}
                    </option>
                  ))}
                </select>
              </div>
            </div>

            <div className="pca-row pca-row-3">
              <div className="pca-field">
                <label className="req" htmlFor="pca-date">
                  Meeting Date
                </label>
                <input
                  id="pca-date"
                  type="date"
                  value={meetingDate}
                  readOnly
                  required
                  aria-readonly="true"
                />
              </div>
              <div className="pca-field">
                <label className="req" htmlFor="pca-time">
                  Meeting Time
                </label>
                <div className={`pca-time-picker-wrap ${showTimePicker ? "open" : ""}`} ref={timePickerRef}>
                  <div className="pca-time-cst">
                    <input
                      id="pca-time"
                      type="time"
                      step={60}
                      value={meetingTime}
                      onClick={() => setShowTimePicker(true)}
                      onInput={(e) => {
                        const next = (e.target as HTMLInputElement).value;
                        setMeetingTime(next);
                        const mins = inputTimeToMinutes(next);
                        if (mins != null) {
                          setStartMin(zoneToCst(mins, timeZone));
                        }
                      }}
                      onChange={(e) => {
                        const next = e.target.value;
                        setMeetingTime(next);
                        const mins = inputTimeToMinutes(next);
                        if (mins != null) {
                          setStartMin(zoneToCst(mins, timeZone));
                        }
                      }}
                      onBlur={(e) => {
                        const next = e.target.value;
                        const mins = inputTimeToMinutes(next);
                        if (mins != null) {
                          setStartMin(zoneToCst(mins, timeZone));
                        } else if (startMin != null) {
                          setMeetingTime(minsToInputTime(cstToZone(startMin, timeZone)));
                        }
                      }}
                      required
                    />
                    <button
                      type="button"
                      className="pca-time-clock-btn"
                      title="Choose time"
                      aria-label="Choose time"
                      onClick={() => setShowTimePicker((p) => !p)}
                    >
                      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                        <circle cx="12" cy="12" r="10" />
                        <polyline points="12 6 12 12 16 14" />
                      </svg>
                    </button>
                    <select
                      className="pca-time-zone"
                      value={timeZone}
                      aria-label="Time zone"
                      onChange={(e) => {
                        const next = parseUsZone(e.target.value);
                        setTimeZone(next);
                        const activeCst =
                          startMin != null
                            ? startMin
                            : inputTimeToMinutes(meetingTime) != null
                              ? zoneToCst(inputTimeToMinutes(meetingTime)!, timeZone)
                              : null;
                        if (activeCst != null) {
                          setStartMin(activeCst);
                          setMeetingTime(minsToInputTime(cstToZone(activeCst, next)));
                        }
                      }}
                    >
                      {US_ZONES.map((z) => (
                        <option key={z} value={z}>
                          {z}
                        </option>
                      ))}
                    </select>
                  </div>

                  {showTimePicker && (
                    <div className="pca-time-popup" role="listbox" aria-label="Available times">
                      <div className="pca-time-popup-head">Select time ({timeZone})</div>
                      <div className="pca-time-popup-scroll">
                        {TIME_OPTIONS.map((slotMins) => {
                          const activeLocal =
                            startMin != null
                              ? cstToZone(startMin, timeZone)
                              : inputTimeToMinutes(meetingTime);
                          const isSel = activeLocal === slotMins;
                          return (
                            <button
                              key={slotMins}
                              type="button"
                              className={`pca-time-popup-opt ${isSel ? "active" : ""}`}
                              onClick={() => {
                                setStartMin(zoneToCst(slotMins, timeZone));
                                setMeetingTime(minsToInputTime(slotMins));
                                setShowTimePicker(false);
                              }}
                            >
                              <span>{minsToTimeStr(slotMins)}</span>
                              {isSel && <span className="pca-time-tick">✓</span>}
                            </button>
                          );
                        })}
                      </div>
                    </div>
                  )}
                </div>
              </div>
              <div className="pca-field">
                <label className="req" htmlFor="pca-duration">
                  Meeting Duration
                </label>
                <select
                  id="pca-duration"
                  value={meetingDuration}
                  onChange={(e) => setMeetingDuration(e.target.value)}
                  required
                >
                  <option value="">Select duration</option>
                  {DURATIONS.map((d) => (
                    <option key={d} value={d}>
                      {d}
                    </option>
                  ))}
                </select>
              </div>
            </div>

            <div className="pca-row pca-row-3">
              <div className="pca-field">
                <label className="req" htmlFor="pca-client">
                  Client
                </label>
                <input
                  id="pca-client"
                  type="text"
                  value={client}
                  onChange={(e) => setClient(e.target.value)}
                  required
                />
              </div>
              <div className="pca-field">
                <label htmlFor="pca-vendor">Vendor</label>
                <input
                  id="pca-vendor"
                  type="text"
                  value={vendor}
                  onChange={(e) => setVendor(e.target.value)}
                />
              </div>
              <div className="pca-field">
                <label className="req" htmlFor="pca-panel">
                  Panel
                </label>
                <input
                  id="pca-panel"
                  type="text"
                  value={panel}
                  onChange={(e) => setPanel(e.target.value)}
                  required
                />
              </div>
            </div>

            <div className="pca-row pca-row-2 pca-row-resume-note">
              <ResumeFileField
                id="pca-resume"
                file={resumeFile}
                onChange={setResumeFile}
                onTooBig={() =>
                  status.show(
                    "error",
                    "Use a PDF or Word file under 5 MB.",
                    "File too big"
                  )
                }
              />
              <div className="pca-field">
                <label htmlFor="pca-note">Note / Meeting URL</label>
                <input
                  id="pca-note"
                  type="text"
                  placeholder="Optional note or meeting URL"
                  value={specialNote}
                  onChange={(e) => setSpecialNote(e.target.value)}
                />
              </div>
            </div>

            <div className="pca-field">
              <label className="req" htmlFor="pca-jd">
                Job Description
              </label>
              <textarea
                id="pca-jd"
                placeholder="Paste the job description text"
                value={jobDescription}
                onChange={(e) => setJobDescription(e.target.value)}
                required
                rows={4}
              />
            </div>

            <button
              className="pca-book-submit"
              type="submit"
              disabled={submitting || hold.reserving || (!slot.emergency && !hold.holdId)}
            >
              {submitting
                ? "Submitting…"
                : hold.reserving
                  ? "Checking slot…"
                  : "Submit Phone Call Request"}
            </button>
          </form>
        ) : null}
      </div>
      <StatusToast toast={status.toast} onDismiss={status.dismiss} />
    </div>
  );
}
