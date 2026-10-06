# Production data flow

**Site:** https://cubic-data.com

This document describes how production data moves among the website, Google Sheets, Drive, Gmail, Calendar, and MongoDB. Spreadsheets are the system of record. The website is the operator surface.

```
Staff / candidate
        │
        ▼
https://cubic-data.com
        │
        ├── Google Sheets     system of record
        ├── MongoDB           website snapshot + in-flight booking queue
        ├── Google Drive      resume and job-description files
        ├── Gmail             booking confirmation
        └── Calendar          accepted phone-call events
```

---

## Google identities

Three credentials. Each has a fixed job. They are not interchangeable.

| Identity | Role |
|---|---|
| **sheets-automation@…** (service account) | Read and write spreadsheets. List Drive folders. Receive Drive change notifications. |
| **karki@cubicit.net** (user OAuth) | Send Gmail. Write Calendar. Upload files into Drive. |
| **Cubic Data Dashboard** (web OAuth client) | Google sign-in for the website. Does not write Sheets, Drive, or Calendar. |

Sheets access can succeed while mail, Calendar, or resume upload fail. That is the karki@ token, not the service account.

Secrets (`AUTH_JWT_SECRET`, OAuth tokens, `CRON_SECRET`, `MONGODB_URI`) live in **`.env` / `.env.production`** on the VPS. A deploy does not replace those files.

---

## System of record vs website snapshot

**Google Sheets are the system of record** for bookings, roster, apply counts, hiring links, and the Do Not Apply list. Operators edit the workbooks. The website writes new bookings and apply counts into those same workbooks.

**MongoDB** (`MONGODB_URI`, database `cubic_data`) holds a snapshot the website can serve without a Google round-trip on every page load, plus the in-flight booking queue.

```
Edit in the sheet
        │
        ▼
Google Drive calls https://cubic-data.com/api/webhooks/drive-changes
        │
        ▼
Site re-reads that workbook by header title
        │
        ▼
MongoDB snapshot replaced
        │
        ▼
Open hiring page or Do Not Apply list picks it up
(about every 5 seconds; no manual refresh)
```

Only two workbooks raise that webhook. The site keeps the watch registered, so an edit itself is the trigger. The call carries no cell values. The site then reads the sheet and replaces the snapshot.

| Sheet that was edited | Webhook | What the site refreshes | What the visitor sees |
|---|---|---|---|
| Application tracking (`Apply_Links `, and the fallback block list) | Drive calls the site at once | Hiring links. The block list is re-read; Cubic_Interview `Do_NOT_Apply` still wins when it has rows | `/hiring` and the home list, within about 5 seconds |
| DATA Candidate (`Current_Market` personal emails) | Drive calls the site at once | Sign-in list | The next sign-in |
| Cubic_Interview `Do_NOT_Apply` | Not watched | The next 5-minute backup | Home list within about 5 seconds after that backup |
| Connector, `Phone calls`, tracker month tabs, Cubic_Interview month tabs | Not snapshotted | Read from Google when that page loads | That page, on load |

If a Drive call is missed, the same re-read runs every **5 minutes**. An empty read does not replace a snapshot that already has rows.

A booking the site writes does not wait on this webhook. Drive and the sheet row are written from the booking queue.

| Mongo | Source workbook / tab | Website surface |
|---|---|---|
| `site_cache` / `hiring-postings` | Application tracking → `Apply_Links ` | `/hiring` |
| `site_cache` / `do-not-apply` | Cubic_Interview → `Do_NOT_Apply`, else application tracking → `Do_Not_Apply` | Home-page list |
| `site_cache` / `allowed-users` | DATA Candidate → `Current_Market` (personal emails) | Sign-in gate |
| `site_cache` / `hiring-job-meta` | Derived job-card fields for hiring links | `/hiring` |
| `booking_queue` | In-flight interview / phone writes | Held until Drive + sheet row exist |

Do not edit Mongo to correct live data. Edit the sheet.

---

## Column mapping

Every connected tab is addressed by the **header title**.

The site locates `Candidate`, `POC`, `Date`, `Time`, and the rest by the text in the header row. Inserting or reordering columns does not change the mapping. A missing required header fails the write. A missing optional header skips that field.

Hidden URL backups occupy the unnamed columns immediately after the last named header.

Header aliases the site already accepts are listed with each tab below. Renaming a title to something unrecognized skips that field, or refuses the write when the header is required.

New booking rows **append** under the last filled data row. Do not insert a row at the top of a Connector or Phone calls tab.

---

## Workbooks

| Spreadsheet | Production use |
|---|---|
| **Google Form To Sheet Connector** | Interview bookings. One tab per Nepal POC: `Prasanna`, `Sajit`, `Saksham`. Data from **row 4**. Accepted interviews are copied to `COPY_TO_CUBIC_SHEET`. |
| **Data Interview Sheet** | Phone-call bookings on `Phone calls` (data from **row 4**). Bound Apps Script writes Accepted rows to Calendar. |
| **DATA Candidate Sheet** | Roster tab `Current_Market`. Header row 1, blank row 2, people from **row 3**. |
| **Data application tracking sheet** | Month tabs (`October_2026`, …) for apply counts. `Apply_Links ` (trailing space is part of the tab name) for hiring links. `Do_Not_Apply` is the fallback block list. `Client List - Interview recieved` feeds `/interviews` (Cubic Interviews), stored as `site_cache` / `cubic-interviews`; refreshed by the daily job, the sheet watcher, and `/api/jobs/cubic-interviews-sync`. |
| **Cubic_Interview Sheet** | Month tabs. Interview availability also counts `Date` / `Time` / `Duration` on these tabs. Tab `Do_NOT_Apply` is the block list the home page uses when it has rows. |

**Otter Attendance - Data** (`ATTENDANCE_SPREADSHEET_ID`): month tabs (`October`, …) with Candidate, Email, then a checkbox per weekday. The live server refreshes this month's roster once a day from the tracking sheet's month tab (adds new candidates, fills emails, applies renames; never deletes rows), and every 5 minutes reads ended calls on Meet `qvf-yine-evq` (karki@ token, `meetings.space.readonly`) and ticks anyone with `ATTENDANCE_MIN_MINUTES` (default 15) that day. Every participant is written to `Attendance Log`, which is also how already-processed calls are skipped. Manual runs: `/api/jobs/otter-roster-sync`, `/api/jobs/otter-attendance?days=7`.

- Connector: https://docs.google.com/spreadsheets/d/1Bg8P3OYHkYRxCDIpISmk0wjz4Q7AcPMT-_4lv-ODOjI
- Data Interview: https://docs.google.com/spreadsheets/d/1o50E_oTIohhsKU0sgNjhPeIAeRpLuoy_rN12ocWwOak
- DATA Candidate: https://docs.google.com/spreadsheets/d/1ixgOU3tvXOmla7CjKiQru_7CZdiI_SsOHomduH1_mrA
- Application tracking: https://docs.google.com/spreadsheets/d/1dFherC2TWbFJFPtwWPHLe57y6iRc_dMUNAXe_ZcWN_A
- Cubic interview schedule: https://docs.google.com/spreadsheets/d/1EcYPqHopEpHt5NiQ9pemVesZav9LtdimPsEp-Z38weU

---

## Drive

Root: **Resumes_Data** — https://drive.google.com/drive/folders/1ttIxh68ovFTlHYcSquTv1n5B-y_ygQsO

Each completed booking writes:

```
Resumes_Data /
  {Candidate Name} /
    {Client} /
      {Interview stage  |  Phone call} /
        Resume.pdf
        Job_Description.txt
```

The booking name typeahead is the set of folder names under `Resumes_Data`. A folder is not sufficient to book: the person must also have a **Nepal POC** on Current_Market.

The resume is a PDF or Word file, 5 MB or smaller. The job description is saved as `Job_Description.txt`.

---

## Current_Market

Looked up by header name.

| Header | Role |
|---|---|
| Full Name | Match the name on the booking form |
| Nepal POC | Connector tab for an interview (`Prasanna` / `Sajit` / `Saksham`) |
| Email (Personal) | Receipt **To**, the candidate’s sign-in address, Application Tracker identity |
| status | Visa value written on the phone-call row |
| Marketing Location | Default location when the form leaves it blank |

The Connector tab comes from **Nepal POC**, not from the Panel field on the form.

| Nepal POC | Connector tab |
|---|---|
| Starts with `pras` | `Prasanna` |
| Starts with `sajit` | `Sajit` |
| Starts with `saksham` or `shaksham` | `Saksham` |
| Anything else | `Prasanna` |

No Nepal POC → the booking is refused.  
No personal email → sheet and Drive still write; the receipt mail is skipped.  
A red-filled row is left off the Application Tracker roster. Booking still matches that person by name.

---

## Access

```
https://cubic-data.com  →  Continue with Google
        │
        ▼
Google account picker
        │
        ▼
https://cubic-data.com/api/auth/callback
        │
        ▼
Session  →  candidate pages, staff pages, or admin
```

Required Google Cloud web-client redirect URI:

`https://cubic-data.com/api/auth/callback`

### User access

A candidate is admitted when their personal email is on **Current_Market** (`Email (Personal)`). The site copies those addresses into the sign-in list (Mongo `allowed-users`). Drive notification and the 5-minute backup refresh that list. They sign in with that Google account and see only their own Application Tracker.

### Staff access

Any `@cubicit.net` Google account can sign in. No Current_Market row is required. On the Application Tracker, staff select a candidate and see that person’s applies, interviews, phone calls, and Drive links.

### Admin access

https://cubic-data.com/admin is a separate list. In production it is **karki@cubicit.net**. That account opens the admin pages (resume templates and the editor). Other `@cubicit.net` accounts stay staff: they use the tracker roster and do not open admin.

---

## Interview booking

**Screen:** https://cubic-data.com/book/appointment

```
Form
        │
        ▼
Reserve the slot if still free
  (max 3 interviews in the same Chicago hour)
        │
        ▼
Current_Market  →  Nepal POC, email, location
        │
        ├── Drive    Resumes_Data / Name / Client / {stage}
        ├── Sheets   Connector / {POC tab}   append, Status = Pending
        │              (Mongo booking_queue until Drive + sheet finish)
        └── Gmail    confirmation to the candidate, CC the team
```

Availability also counts bookings still in the queue, so two browsers cannot take the last slot together.

The page confirms as soon as the request is queued. Drive, the sheet row, and the receipt mail finish immediately after that. A failed attempt is tried again, up to 5 times. The same candidate, client, stage, date, and time is not appended twice. If the row is written and the mail fails, the row stays.

### Connector POC tab

| Header | Source |
|---|---|
| Submitted / Timestamp | Server, Chicago time |
| Candidate / Candidate name / Full name | Form |
| Interview stage / Stage | Form |
| Location / Marketing location | Form, else Current_Market |
| Mode / Platform | Form |
| Nepal POC / POC | Current_Market Nepal POC |
| Meeting date / Date | Form |
| Meeting time / Time (CST) / Time | Form, CST label |
| Duration | Form |
| Client | Form |
| Vendor | Form |
| Panel | Form |
| Status | `Pending` on create |
| Special note / Note | Form |
| Resume Link / Resume | Drive |
| Job Description Link / JD | Drive |
| Drive Folder Link / Folder | Drive |
| Email (Personal) / Email | Current_Market |
| (unnamed backups) | Hidden raw URLs, right of the last named header |

Meeting time on the form includes AM/PM (preferably `CST`). A bare `08:00` is interpreted as evening for hours 1–8.

### Accepted interviews

When Status becomes **Accepted**, the status sync copies that row onto Connector tab **`COPY_TO_CUBIC_SHEET`** by header name: POC, Client, Tech, Location, Visa, Candidate, Date, Time, Duration, Panel, Note. The sync runs about every **10 minutes**, and again on the **7:00 AM Chicago** daily run. That tab is the feed into the Cubic schedule. Interviews are not placed on karki@ Calendar.

---

## Phone-call booking

**Screen:** https://cubic-data.com/book/phone-call

Same Drive tree; the stage folder is always `Phone call`. The sheet row goes to **Data Interview → `Phone calls`**. Same booking queue until the write completes.

Capacity: **1** booking per 30-minute Chicago slot.

### Phone calls tab

| Header | Source |
|---|---|
| Nepal POC / POC | Current_Market |
| Client | Form |
| Tech | Current_Market, else `Data` |
| Location | Form, else Current_Market |
| Visa / status | Current_Market `status` |
| Candidate | Form |
| Date | Form |
| Time | Form |
| Duration | Form |
| Panel | Form |
| Status | `Pending` on create |
| Job Description Link / JD | Drive |
| Resume Link / Resume | Drive |
| Drive Folder Link / Folder | Drive |
| Special note / Note | Form |
| Email | Current_Market |
| (unnamed backups) | Hidden raw URLs, right of the last named header |

A row counts as a booking when **Candidate**, **Client**, or **Date** is filled. The next append is the first empty data row, not a Support cell dragged down the column.

Phone-call rows older than **7 days** are removed by the Apps Script on the Data Interview sheet. The website does not delete them.

---

## Calendar

Calendar: **karki@cubicit.net** primary calendar.

| Event | Trigger | Writer |
|---|---|---|
| Phone call created | `Phone calls` Status → **Accepted** | Apps Script on Data Interview (`phoneCallCalendar`) |
| Phone call removed | Status leaves Accepted | Same script |
| Interview | — | Not written to this calendar; see `COPY_TO_CUBIC_SHEET` |

Phone-call event:

- Title: `Phone call: {candidate} — {client}`
- Time zone: America/Chicago
- Always invited: `sushantmaharjan@cubicit.net`
- Also invited: support person on the row, when an email is known
- Attachments: Resume and JD Drive links when those cells are filled

Production Calendar writes stay on that Apps Script. The website does not write the same events. A working karki@ login is what uploads the resume; it does not by itself put an interview on the calendar. An event exists only after a `Phone calls` row is **Accepted**.

---

## Confirmation email

Sent after the booking row exists **and** Current_Market has a personal email. This mail says the request was received. It is not an acceptance, and it does not create a calendar event. Acceptance or decline is a later mail. If sending fails after the row exists, the row stays.

| | |
|---|---|
| From | Cubic Interview Team `<reminder@cubicit.net>` |
| To | Candidate personal email |
| CC | `prasanna@cubicit.net`, `sajit@cubicit.net`, `saksham@cubicit.net`, `sushantmaharjan@cubicit.net` |
| Reply-To | `karki@cubicit.net` |

---

## Availability grids

Time zone: America/Chicago. The bookable horizon starts **tomorrow**.

| Screen | Busy-slot sources | Cap |
|---|---|---|
| Interview calendar | Connector `Prasanna` + `Sajit` + `Saksham`, Cubic_Interview month tabs (`Date` / `Time` / `Duration`), Mongo booking queue | 3 per hour |
| Phone calendar | `Phone calls` (`Date` / `Time` / `Duration` / `Status`), Mongo booking queue | 1 per 30 min |
| Name typeahead | Folder names under Resumes_Data | — |

---

## Application Tracker

**Screen:** https://cubic-data.com/tracker

| Session | View |
|---|---|
| Candidate (personal email on Current_Market) | Own apply counts, interviews, phone calls, Drive links |
| Staff `@cubicit.net` | Selects a candidate |

### Today’s applies

One cell on the current month tab (`October_2026`, …):

```
Tracker
        │
        ▼
Month tab  →  candidate row  ×  today’s day column
```

Row match: headers **`Candidate Name`** and **`Mail ( Personal)`**. Day columns: numbered headers (`1`, `2`, …). A missing person gets a new row with those two headers filled. Charts re-read every month tab.

Typical month-tab layout (mapping is still by title):

- Row 1 — weekday labels over the day numbers
- Row 2 — `Candidate Name` \| `Mail ( Personal)` \| day `1` \| `2` \| …
- Rows 3–4 — spacers
- Row 5 onward — one row per candidate

### Interviews and phone calls on the tracker

Read from Connector POC tabs and `Phone calls`, matched by name and/or email. Historical interviews on Data Interview month tabs are matched by name, using the same headers (`Candidate`, `Client`, `Date`, `Time`, …). Resume and JD on company cards are the hyperlinks already on those rows.

---

## Job Hiring

**Screen:** https://cubic-data.com/hiring

System of record: application tracking tab **`Apply_Links `** (trailing space). The page reads the Mongo snapshot of that tab.

| Header | Meaning |
|---|---|
| Apply link / Apply URL / Job link / URL / Link | Job URL |
| Added by / POC | Who posted it |

An edit in that workbook makes Drive call the site. The site re-reads the tab and replaces the snapshot. Nothing is polled on a timer for this. An open page picks up the new snapshot about every **5 seconds**. A missed Drive call is covered by the 5-minute backup.

---

## Do Not Apply

Home-page list. System of record: Cubic_Interview tab **`Do_NOT_Apply`** (`Company` / `Reason`). If that tab has no companies, the site uses application tracking tab **`Do_Not_Apply`**.

The page reads the Mongo snapshot and re-fetches it about every 5 seconds while the list is open. Cubic_Interview is not on the Drive watch, so an edit on `Do_NOT_Apply` shows up on the next 5-minute backup. An edit in the application tracking workbook refreshes the snapshot immediately, and that refresh still prefers `Do_NOT_Apply` when it has rows.

---

## End-to-end

### Interview or phone request

```
https://cubic-data.com/book/…
        │
        ▼
Mongo booking_queue
        │
        ▼
Current_Market     POC + email + visa     (header names)
        │
        ├────────── Drive      Resumes_Data / Name / Client / Stage
        │
        ├────────── Sheets     append by header name
        │                 interview → Connector / {POC tab}     Status Pending
        │                 phone     → Data Interview / Phone calls
        │
        └────────── Gmail
                        To: candidate
                        CC: POCs + Sushant
                        From: reminder@cubicit.net
```

On the sheet, later:

```
Status → Accepted
        │
        ├── interview  →  COPY_TO_CUBIC_SHEET
        │                 (status sync, about every 10 minutes)
        └── phone      →  karki@ calendar event
                          (Apps Script on Data Interview, not the website)
```

### Today’s apply count

```
https://cubic-data.com/tracker
        │
        ▼
Tracking sheet / {Month}_{Year}
        row    = Candidate Name + Mail headers
        column = today’s day-number header
```

---

## Operating constraints

1. Map every tab by **header titles**.
2. **Append** on Connector and Phone calls; do not insert at the top.
3. Meeting time includes AM/PM (`8 AM CST`).
4. The receipt mail and the calendar event are separate. Mail goes out when the row is written. A calendar event exists only after a phone-call row is Accepted. Confirm each with a real send, not a token probe.
5. Laptop OAuth tokens and VPS tokens are separate. Redeploy does not copy Drive / Gmail / Calendar secrets.
6. One Calendar writer for phone calls: the Data Interview Apps Script.
7. Sheets are the system of record. Mongo is the website snapshot and the in-flight booking queue. Correct data on the sheet.
8. Keep header names stable. An unrecognized title is skipped, or the write is refused when that header is required.
9. Each production UI build has a version id. Pages are not browser-cached; an open tab reloads when `/api/version` is newer than the bundle it is running.
10. Nepal POC chooses the Connector tab. The Panel field is only a column on the row.
11. Phone-call rows older than 7 days are removed by the Data Interview script.
