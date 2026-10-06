use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct Config {
    pub port: u16,
    pub jwt_secret: String,
    pub allow_any_google: bool,
    pub allowed_domain: String,
    pub extra_emails: Vec<String>,
    /// May sign in, but with candidate-level access only (never staff views).
    /// AUTH_CANDIDATE_EMAILS, comma separated.
    pub candidate_emails: Vec<String>,
    /// Superadmins — who may open /admin. Deliberately narrower than
    /// `allowed_domain` / `extra_emails`: those grant the Application Tracker's
    /// staff roster view, this grants the admin portal. Override with
    /// SUPERADMIN_EMAILS.
    pub superadmin_emails: Vec<String>,
    /// ONLYOFFICE Docs — the browser loads the editor from this origin, so it
    /// must be publicly reachable, not a loopback address.
    pub onlyoffice_url: String,
    /// Shared secret the Document Server signs/verifies with. Empty disables the
    /// editor rather than silently serving configs the server will reject.
    pub onlyoffice_jwt_secret: String,
    /// Origin the Document Server uses to reach *us*. Not the browser's origin:
    /// on the VPS the container reaches the host at 172.17.0.1, and under colima
    /// it is host.docker.internal. Empty falls back to site_url.
    pub onlyoffice_callback_origin: String,
    pub site_url: String,
    pub cron_secret: String,
    pub booking_use_apps_script: bool,
    pub google_oauth_client_id: String,
    pub google_oauth_client_secret: String,
    pub oauth_web_path: Option<PathBuf>,
    pub service_account_path: PathBuf,
    pub gmail_oauth_path: Option<PathBuf>,
    pub calendar_oauth_path: Option<PathBuf>,
    pub gmail_client_id: String,
    pub gmail_client_secret: String,
    pub gmail_refresh_token: String,
    pub calendar_client_id: String,
    pub calendar_client_secret: String,
    pub calendar_refresh_token: String,
    pub data_dir: PathBuf,
    /// MongoDB URI for the fast website copy of sheet lists + booking queue.
    pub mongodb_uri: String,
    /// Seconds between backup sheet → Mongo pulls if a Drive notification is missed. `0` off.
    pub sheet_watch_secs: u64,
    /// Local/dev only: Application Tracker loads this candidate instead of the session email.
    pub tracker_mock_email: String,
    pub connector_spreadsheet_id: String,
    pub resumes_data_folder_id: String,
    pub data_interview_spreadsheet_id: String,
    pub data_candidate_spreadsheet_id: String,
    pub hiring_spreadsheet_id: String,
    pub do_not_apply_tab: String,
    /// "Otter Attendance - Data" workbook: one tab per month, a checkbox per weekday.
    pub attendance_spreadsheet_id: String,
    /// Meet code of the Otter & Pronunciation class (organized by the calendar OAuth user).
    pub otter_meet_code: String,
    /// Minutes in the call (either session) that count as present.
    pub attendance_min_minutes: i64,
    pub do_not_apply_source_tab: String,
    pub appointment_spreadsheet_id: String,
    pub cubic_tab: String,
    pub backend_state_sheet: String,
    pub poc_sheets: Vec<String>,
    pub phone_calls_sheet: String,
    pub timezone: String,
    pub status_default: String,
    pub connector_data_start_row: i32,
    pub phone_calls_retention_days: i64,
    /// Rust rewrites the Phone calls tab for retention only when
    /// `RUST_PHONE_CALLS_PRUNE` is on. The Apps Script `prunePhoneCallsRetentionDaily`
    /// already does it at the same 7 AM, and also carries row colours and the
    /// row-keyed status-email flags across the move, which this job cannot. Two
    /// rewrites racing on one tab shifted rows away from their links (2026-09).
    pub rust_phone_calls_prune: bool,
    pub calendar_id: String,
    pub calendar_always_guests: Vec<String>,
    pub calendar_owner_email: String,
    /// The Apps Script `phoneCallCalendar.gs` bound to the Data Interview sheet
    /// is the authoritative writer of phone-call calendar events (it has its own
    /// dedup, attachments, decline-removal). This backend only writes them when
    /// `RUST_PHONE_CALENDAR` is explicitly turned on — otherwise both writers
    /// fire and every booking lands on the calendar twice.
    pub rust_phone_calendar: bool,
    pub mail_from_email: String,
    pub mail_sender_name: String,
    /// When set, every outbound booking email goes here instead of the real
    /// recipient/CC — for safe end-to-end testing. Unset in normal operation.
    pub test_email: Option<String>,
    pub support_staff_emails: HashMap<String, String>,
    pub poc_emails: HashMap<String, String>,
    pub ui_origin: String,
    pub static_dir: Option<PathBuf>,
}

#[derive(Deserialize)]
struct ServiceAccountFile {
    client_email: String,
    private_key: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServiceAccount {
    pub client_email: String,
    pub private_key: String,
}

#[derive(Deserialize, Default)]
struct OauthFile {
    client_id: Option<String>,
    client_secret: Option<String>,
    refresh_token: Option<String>,
    web: Option<OauthPair>,
    installed: Option<OauthPair>,
}

#[derive(Deserialize, Default)]
struct OauthPair {
    client_id: Option<String>,
    client_secret: Option<String>,
}

fn env_flag(raw: Option<String>, default_on: bool) -> bool {
    match raw {
        None => default_on,
        Some(v) => {
            let v = v.trim().to_lowercase();
            if v.is_empty() {
                default_on
            } else {
                v != "false" && v != "0" && v != "no"
            }
        }
    }
}

fn first_existing(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.into_iter().find(|p| p.is_file()).cloned()
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn load() -> Config {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let site_dir = crate_dir.parent().unwrap_or(&crate_dir).to_path_buf();
    let repo_dir = site_dir.parent().unwrap_or(&site_dir).to_path_buf();

    // Most specific file first: dotenvy does not override vars already set.
    // .env.local (dev) \u2192 .env \u2192 .env.production. Systemd EnvironmentFile
    // still wins on the VPS because those vars are already in the process env.
    for dir in [&site_dir, &repo_dir, &crate_dir] {
        let _ = dotenvy::from_filename(dir.join(".env.local"));
        let _ = dotenvy::from_filename(dir.join(".env"));
        let _ = dotenvy::from_filename(dir.join(".env.production"));
    }

    let jwt_secret = env::var("AUTH_JWT_SECRET")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if env::var("VERCEL").is_ok() || env::var("NODE_ENV").ok().as_deref() == Some("production")
            {
                panic!("AUTH_JWT_SECRET is required in production.");
            }
            "cubic-dev-jwt-secret-do-not-use-in-prod".into()
        });

    let extras = env::var("AUTH_EXTRA_EMAILS")
        .unwrap_or_default()
        .split(',')
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty())
        .collect();

    let candidate_emails = env::var("AUTH_CANDIDATE_EMAILS")
        .unwrap_or_else(|_| "shaksham78karki@gmail.com".into())
        .split(',')
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty())
        .collect();

    let superadmin_emails = env::var("SUPERADMIN_EMAILS")
        .unwrap_or_else(|_| "karki@cubicit.net".into())
        .split(',')
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty())
        .collect();

    let service_account_path = env::var("GOOGLE_SERVICE_ACCOUNT_KEY")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            first_existing(&[
                site_dir.join("service-account-key.json"),
                repo_dir.join("service-account-key.json"),
                crate_dir.join("service-account-key.json"),
            ])
        })
        .unwrap_or_else(|| site_dir.join("service-account-key.json"));

    let oauth_web_path = first_existing(&[
        site_dir.join("oauth-web.json"),
        repo_dir.join("oauth-web.json"),
    ]);
    let gmail_oauth_path = env::var("GMAIL_OAUTH_JSON")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            first_existing(&[
                site_dir.join("gmail-oauth.json"),
                repo_dir.join("gmail-oauth.json"),
            ])
        });
    let calendar_oauth_path = env::var("CALENDAR_OAUTH_JSON")
        .ok()
        .map(PathBuf::from)
        // A stale env path (points at a file that isn't there) must not shadow
        // the real token file next to it.
        .filter(|p| p.is_file())
        .or_else(|| {
            first_existing(&[
                site_dir.join(".calendar_oauth_token.json"),
                site_dir.join("calendar-oauth.json"),
                repo_dir.join(".calendar_oauth_token.json"),
            ])
        });

    let mut google_oauth_client_id = env::var("GOOGLE_OAUTH_CLIENT_ID")
        .or_else(|_| env::var("GMAIL_CLIENT_ID"))
        .unwrap_or_default()
        .trim()
        .to_string();
    let mut google_oauth_client_secret = env::var("GOOGLE_OAUTH_CLIENT_SECRET")
        .or_else(|_| env::var("GMAIL_CLIENT_SECRET"))
        .unwrap_or_default()
        .trim()
        .to_string();
    if google_oauth_client_id.is_empty() || google_oauth_client_secret.is_empty() {
        if let Some(path) = &oauth_web_path {
            if let Some(raw) = read_json::<OauthFile>(path) {
                if google_oauth_client_id.is_empty() {
                    google_oauth_client_id = raw
                        .web
                        .as_ref()
                        .and_then(|w| w.client_id.clone())
                        .unwrap_or_default();
                }
                if google_oauth_client_secret.is_empty() {
                    google_oauth_client_secret = raw
                        .web
                        .as_ref()
                        .and_then(|w| w.client_secret.clone())
                        .unwrap_or_default();
                }
            }
        }
    }

    let mut gmail = load_api_oauth(
        env::var("GMAIL_CLIENT_ID").unwrap_or_default(),
        env::var("GMAIL_CLIENT_SECRET").unwrap_or_default(),
        env::var("GMAIL_REFRESH_TOKEN").unwrap_or_default(),
        gmail_oauth_path.as_deref(),
        false,
    );
    let calendar = load_api_oauth(
        // Calendar + Drive-write refresh tokens were issued for the SAME desktop
        // client as Gmail — seed with the Gmail client creds so the calendar
        // token file only has to supply the refresh_token.
        env::var("CALENDAR_CLIENT_ID")
            .or_else(|_| env::var("GMAIL_CLIENT_ID"))
            .unwrap_or_else(|_| gmail.0.clone()),
        env::var("CALENDAR_CLIENT_SECRET")
            .or_else(|_| env::var("GMAIL_CLIENT_SECRET"))
            .unwrap_or_else(|_| gmail.1.clone()),
        env::var("CALENDAR_REFRESH_TOKEN").unwrap_or_default(),
        calendar_oauth_path.as_deref(),
        true,
    );
    if gmail.0.is_empty() {
        gmail.0 = calendar.0.clone();
    }
    if gmail.1.is_empty() {
        gmail.1 = calendar.1.clone();
    }

    let support_staff_emails = HashMap::from([
        ("Piyush".into(), "piyush@cubicit.net".into()),
        ("Subash".into(), "subash@cubicit.net".into()),
        ("Saksham".into(), "shaksham78karki@gmail.com".into()),
        ("Suman".into(), "suman@cubicit.net".into()),
        ("IAMShreya".into(), "shreya@cubicit.net".into()),
        ("Shreya".into(), "shreya@cubicit.net".into()),
        ("Dipesh".into(), "dipesh@cubicit.net".into()),
        ("Sushant".into(), "sushantmaharjan@cubicit.net".into()),
        ("Abhishek".into(), "abhishek@cubicit.net".into()),
        ("Sam".into(), "sam@cubicit.net".into()),
        ("Shivu".into(), "shivu@cubicit.net".into()),
        ("Sameer".into(), "sameer@cubicit.net".into()),
        ("Kshitiz".into(), "kshitiz@cubicit.net".into()),
        ("Nabin".into(), "nabin@cubicit.net".into()),
        ("Saurav".into(), "saurav.nir76@gmail.com".into()),
        ("Prasanna".into(), "prasanna@cubicit.net".into()),
        ("Shubham".into(), "Shubhamshah441@gmail.com".into()),
        ("Nitesh".into(), "njha1999k@gmail.com".into()),
    ]);
    let poc_emails = HashMap::from([
        ("Prasanna".into(), "prasanna@cubicit.net".into()),
        ("Sajit".into(), "sajit@cubicit.net".into()),
        ("Saksham".into(), "saksham@cubicit.net".into()),
    ]);

    Config {
        port: env::var("RUST_BACKEND_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8080),
        jwt_secret,
        // Default false: only @AUTH_ALLOWED_DOMAIN, AUTH_EXTRA_EMAILS, and
        // Current_Market emails (local allowed-users.json / webhook).
        allow_any_google: env_flag(env::var("AUTH_ALLOW_ANY_GOOGLE").ok(), false),
        allowed_domain: env::var("AUTH_ALLOWED_DOMAIN").unwrap_or_else(|_| "cubicit.net".into()),
        extra_emails: extras,
        candidate_emails,
        superadmin_emails,
        onlyoffice_url: env::var("ONLYOFFICE_URL")
            .unwrap_or_default()
            .trim()
            .trim_end_matches('/')
            .to_string(),
        onlyoffice_jwt_secret: env::var("ONLYOFFICE_JWT_SECRET")
            .unwrap_or_default()
            .trim()
            .to_string(),
        onlyoffice_callback_origin: env::var("ONLYOFFICE_CALLBACK_ORIGIN")
            .unwrap_or_default()
            .trim()
            .trim_end_matches('/')
            .to_string(),
        site_url: env::var("AUTH_SITE_URL")
            .or_else(|_| env::var("NEXT_PUBLIC_SITE_URL"))
            .unwrap_or_else(|_| "http://localhost:3000".into())
            .trim_end_matches('/')
            .to_string(),
        cron_secret: env::var("CRON_SECRET")
            .or_else(|_| env::var("JOBS_SECRET"))
            .unwrap_or_default(),
        booking_use_apps_script: env::var("BOOKING_USE_APPS_SCRIPT").ok().as_deref() == Some("true"),
        google_oauth_client_id,
        google_oauth_client_secret,
        oauth_web_path,
        service_account_path,
        gmail_oauth_path,
        calendar_oauth_path,
        gmail_client_id: gmail.0,
        gmail_client_secret: gmail.1,
        gmail_refresh_token: gmail.2,
        calendar_client_id: calendar.0,
        calendar_client_secret: calendar.1,
        calendar_refresh_token: calendar.2,
        data_dir: env::var("CUBIC_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| site_dir.join("data")),
        mongodb_uri: env::var("MONGODB_URI").unwrap_or_else(|_| "mongodb://127.0.0.1:27017/cubic_data".into()),
        sheet_watch_secs: env::var("SHEET_WATCH_SECS")
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(300),
        tracker_mock_email: env::var("CUBIC_TRACKER_MOCK_EMAIL")
            .unwrap_or_default()
            .trim()
            .to_lowercase(),
        connector_spreadsheet_id: env::var("CONNECTOR_SPREADSHEET_ID")
            .unwrap_or_else(|_| "1Bg8P3OYHkYRxCDIpISmk0wjz4Q7AcPMT-_4lv-ODOjI".into()),
        resumes_data_folder_id: env::var("RESUMES_DATA_FOLDER_ID")
            .unwrap_or_else(|_| "1ttIxh68ovFTlHYcSquTv1n5B-y_ygQsO".into()),
        data_interview_spreadsheet_id: env::var("DATA_INTERVIEW_SPREADSHEET_ID")
            .unwrap_or_else(|_| "1o50E_oTIohhsKU0sgNjhPeIAeRpLuoy_rN12ocWwOak".into()),
        data_candidate_spreadsheet_id: env::var("DATA_CANDIDATE_SPREADSHEET_ID")
            .unwrap_or_else(|_| "1ixgOU3tvXOmla7CjKiQru_7CZdiI_SsOHomduH1_mrA".into()),
        hiring_spreadsheet_id: "1dFherC2TWbFJFPtwWPHLe57y6iRc_dMUNAXe_ZcWN_A".into(),
        do_not_apply_tab: env::var("DO_NOT_APPLY_TAB").unwrap_or_else(|_| "Do_Not_Apply".into()),
        attendance_spreadsheet_id: env::var("ATTENDANCE_SPREADSHEET_ID")
            .unwrap_or_else(|_| "1qFyfUPs_XohbGrIqZMgeqVozyYZamG9DaFLNOI0d-5k".into()),
        otter_meet_code: env::var("OTTER_MEET_CODE").unwrap_or_else(|_| "qvf-yine-evq".into()),
        attendance_min_minutes: env::var("ATTENDANCE_MIN_MINUTES")
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(15),
        do_not_apply_source_tab: env::var("DO_NOT_APPLY_SOURCE_TAB")
            .unwrap_or_else(|_| "Do_NOT_Apply".into()),
        appointment_spreadsheet_id: "1EcYPqHopEpHt5NiQ9pemVesZav9LtdimPsEp-Z38weU".into(),
        cubic_tab: "COPY_TO_CUBIC_SHEET".into(),
        backend_state_sheet: "_SiteBackend".into(),
        poc_sheets: vec!["Prasanna".into(), "Sajit".into(), "Saksham".into()],
        phone_calls_sheet: "Phone calls".into(),
        timezone: "America/Chicago".into(),
        status_default: "Pending".into(),
        connector_data_start_row: 4,
        phone_calls_retention_days: 7,
        rust_phone_calls_prune: env::var("RUST_PHONE_CALLS_PRUNE")
            .map(|v| matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "on" | "yes"))
            .unwrap_or(false),
        calendar_id: env::var("GOOGLE_CALENDAR_ID").unwrap_or_else(|_| "primary".into()),
        calendar_always_guests: vec!["sushantmaharjan@cubicit.net".into()],
        calendar_owner_email: "karki@cubicit.net".into(),
        rust_phone_calendar: env::var("RUST_PHONE_CALENDAR")
            .map(|v| matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "on" | "yes"))
            .unwrap_or(false),
        mail_from_email: env::var("GMAIL_FROM_EMAIL").unwrap_or_else(|_| "reminder@cubicit.net".into()),
        test_email: env::var("BOOKING_TEST_EMAIL_OVERRIDE")
            .or_else(|_| env::var("BOOKING_TEST_EMAIL"))
            .ok()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| s.contains('@')),
        mail_sender_name: "Cubic Interview Team".into(),
        support_staff_emails,
        poc_emails,
        ui_origin: env::var("CUBIC_UI_ORIGIN")
            .unwrap_or_default()
            .trim_end_matches('/')
            .to_string(),
        static_dir: env::var("CUBIC_STATIC_DIR")
            .ok()
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| {
                let out = site_dir.join("out");
                out.is_dir().then_some(out)
            }),
    }
}

fn load_api_oauth(
    mut client_id: String,
    mut client_secret: String,
    mut refresh_token: String,
    path: Option<&Path>,
    also_installed: bool,
) -> (String, String, String) {
    client_id = client_id.trim().to_string();
    client_secret = client_secret.trim().to_string();
    refresh_token = refresh_token.trim().to_string();
    if let Some(path) = path {
        if let Some(raw) = read_json::<OauthFile>(path) {
            if client_id.is_empty() {
                client_id = raw
                    .client_id
                    .or_else(|| {
                        if also_installed {
                            raw.installed.as_ref().and_then(|i| i.client_id.clone())
                        } else {
                            None
                        }
                    })
                    .or_else(|| raw.web.as_ref().and_then(|w| w.client_id.clone()))
                    .unwrap_or_default();
            }
            if client_secret.is_empty() {
                client_secret = raw
                    .client_secret
                    .or_else(|| {
                        if also_installed {
                            raw.installed.as_ref().and_then(|i| i.client_secret.clone())
                        } else {
                            None
                        }
                    })
                    .or_else(|| raw.web.as_ref().and_then(|w| w.client_secret.clone()))
                    .unwrap_or_default();
            }
            if refresh_token.is_empty() {
                refresh_token = raw.refresh_token.unwrap_or_default();
            }
        }
    }
    (client_id, client_secret, refresh_token)
}

impl Config {
    pub fn load_service_account(&self) -> anyhow::Result<ServiceAccount> {
        let raw = std::fs::read_to_string(&self.service_account_path).map_err(|err| {
            anyhow::anyhow!(
                "service account key missing at {}: {err}",
                self.service_account_path.display()
            )
        })?;
        let parsed: ServiceAccountFile = serde_json::from_str(&raw)?;
        Ok(ServiceAccount {
            client_email: parsed.client_email,
            private_key: parsed.private_key,
        })
    }

    pub fn cookie_secure(&self) -> bool {
        self.site_url.starts_with("https://") || env::var("VERCEL").ok().as_deref() == Some("1")
    }

    pub fn cookie_domain(&self) -> Option<String> {
        let host = url::Url::parse(&self.site_url).ok()?.host_str()?.to_string();
        let host = host.trim_start_matches("www.");
        if host == "cubic-data.com" {
            Some(".cubic-data.com".into())
        } else {
            None
        }
    }

    /// Domain / extra / emergency open-gate only (no sheet list). Prefer
    /// `access::is_login_allowed` for sign-in.
    pub fn allowed_email(&self, email: &str) -> bool {
        let value = email.trim().to_lowercase();
        if value.is_empty() || !value.contains('@') {
            return false;
        }
        if self.allow_any_google {
            return true;
        }
        if value.ends_with(&format!("@{}", self.allowed_domain.trim().to_lowercase())) {
            return true;
        }
        self.extra_emails.contains(&value) || self.candidate_emails.contains(&value)
    }

    pub fn site_origin_from_request(&self, forwarded_host: Option<&str>, forwarded_proto: Option<&str>) -> String {
        if !self.site_url.is_empty() && !self.site_url.contains("localhost") {
            return self.site_url.clone();
        }
        if let Some(host) = forwarded_host {
            let host = host.split(',').next().unwrap_or(host).trim();
            if !host.is_empty() && !host.starts_with("0.0.0.0") && !host.starts_with("[::]") {
                let proto = forwarded_proto
                    .unwrap_or("http")
                    .split(',')
                    .next()
                    .unwrap_or("http")
                    .trim();
                return format!("{proto}://{host}");
            }
        }
        if self.site_url.is_empty() {
            "https://cubic-data.com".into()
        } else {
            self.site_url.clone()
        }
    }
}
