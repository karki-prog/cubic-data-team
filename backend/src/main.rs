mod access;
mod attendance;
mod ai;
mod admin;
mod auth;
mod availability;
mod booking;
mod config;
mod current_market;
mod dates;
mod db;
mod dna;
mod error;
mod headers;
mod google;
mod hiring;
mod interviews;
mod job_page;
mod jobs;
mod office;
mod plugin;
mod sheet_watch;
mod slot_hold;
mod templates;
mod staff;
mod tracker;
mod types;
mod ui;

use std::net::SocketAddr;

use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
use axum::Router;
use tower_http::trace::TraceLayer;

use access::AccessCache;
use booking::BookingQueue;
use db::Db;
use dna::DnaCache;
use google::GoogleClient;
use hiring::HiringCache;
use interviews::InterviewsCache;
use tracker::HistoryCache;
use slot_hold::SlotHoldStore;

#[derive(Clone)]
pub struct AppState {
    pub cfg: config::Config,
    pub google: GoogleClient,
    pub db: Db,
    pub queue: BookingQueue,
    pub hiring: HiringCache,
    pub interviews: InterviewsCache,
    pub dna: DnaCache,
    pub access: AccessCache,
    pub history: HistoryCache,
    pub holds: SlotHoldStore,
    pub app_version: String,
}

impl AppState {
    async fn new(cfg: config::Config) -> Self {
        let google = GoogleClient::new(cfg.clone());
        let db = Db::connect(&cfg.mongodb_uri).await;
        db.migrate_json_file("allowed-users", &cfg.data_dir.join("allowed-users.json"))
            .await;
        db.migrate_json_file("do-not-apply", &cfg.data_dir.join("do-not-apply.json"))
            .await;
        db.migrate_json_file("hiring-postings", &cfg.data_dir.join("hiring-postings.json"))
            .await;
        db.migrate_json_file(
            "hiring-job-meta",
            &cfg.data_dir.join("hiring-job-meta-cache.json"),
        )
        .await;
        let app_version = ui::load_app_version(&cfg);
        Self {
            cfg,
            google,
            db,
            queue: BookingQueue::new(),
            hiring: HiringCache::new(),
            interviews: InterviewsCache::new(),
            dna: DnaCache::new(),
            access: AccessCache::new(),
            history: HistoryCache::new(),
            holds: SlotHoldStore::new(),
            app_version,
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cubic_backend=info,tower_http=info".into()),
        )
        .init();

    let cfg = config::load();
    let port = cfg.port;
    let state = AppState::new(cfg).await;
    sheet_watch::spawn(state.clone());
    attendance::spawn(state.clone());

    let app = Router::new()
        .route("/api/auth/google", get(auth::google_start))
        .route("/api/auth/callback", get(auth::google_callback))
        .route("/api/auth/login", post(auth::login_disabled))
        .route("/api/auth/me", get(auth::me))
        .route("/api/admin/me", get(admin::me))
        .route("/office-plugin", get(plugin::base))
        .route("/office-plugin/{*path}", get(plugin::asset))
        .route("/api/admin/ai/sections", get(ai::sections))
        .route("/api/admin/ai/draft", post(ai::draft))
        .route("/api/admin/ai/draft/stream", post(ai::draft_stream))
        .route("/api/admin/office/editor-config", get(office::editor_config))
        .route(
            "/api/admin/templates",
            get(templates::list).post(templates::create),
        )
        .route("/api/admin/templates/{id}", delete(templates::remove))
        .route("/api/admin/templates/{id}/file", get(templates::download))
        .route(
            "/api/admin/templates/{id}/callback",
            post(templates::save_callback),
        )
        .route("/api/auth/logout", post(auth::logout_post).get(auth::logout_get))
        .route("/api/auth/refresh", post(auth::refresh))
        .route("/api/appointment-book", post(booking::appointment_book))
        .route("/api/phone-call-book", post(booking::phone_call_book))
        .route("/api/slot-hold", post(slot_hold::acquire))
        .route("/api/slot-hold/heartbeat", post(slot_hold::heartbeat))
        .route("/api/slot-hold/release", post(slot_hold::release))
        .route("/api/appointment-availability", get(availability::appointment_availability))
        .route("/api/phone-call-availability", get(availability::phone_call_availability))
        .route("/api/phone-call-candidates", get(availability::phone_call_candidates))
        .route("/api/job-postings", get(hiring::job_postings))
        .route("/api/webhooks/job-postings", post(hiring::webhook))
        .route("/api/webhooks/drive-changes", post(sheet_watch::drive_changes))
        .route(
            "/api/jobs/job-postings-sync",
            get(hiring::sync).post(hiring::sync),
        )
        .route("/api/cubic-interviews", get(interviews::cubic_interviews))
        .route("/api/webhooks/cubic-interviews", post(interviews::webhook))
        .route(
            "/api/jobs/cubic-interviews-sync",
            get(interviews::sync).post(interviews::sync),
        )
        .route("/api/otter-attendance", get(attendance::view_route))
        .route(
            "/api/jobs/otter-roster-sync",
            get(attendance::roster_route).post(attendance::roster_route),
        )
        .route(
            "/api/jobs/otter-attendance",
            get(attendance::sync_route).post(attendance::sync_route),
        )
        .route("/api/do-not-apply", get(dna::do_not_apply))
        .route("/api/webhooks/do-not-apply", post(dna::webhook))
        .route(
            "/api/jobs/do-not-apply-sync",
            get(dna::sync).post(dna::sync),
        )
        .route("/api/webhooks/allowed-users", post(access::webhook))
        .route(
            "/api/jobs/allowed-users-sync",
            get(access::sync).post(access::sync),
        )
        .route("/api/application-tracker", get(tracker::application_tracker))
        .route("/api/application-tracker/applies", post(tracker::log_today_applies))
        .route("/api/jobs/daily", get(jobs::daily).post(jobs::daily))
        .route("/api/jobs/status-sync", get(jobs::status_sync).post(jobs::status_sync))
        .route("/api/jobs/booking-drain", get(jobs::booking_drain).post(jobs::booking_drain))
        .route(
            "/api/jobs/phone-calls-prune",
            get(jobs::phone_calls_prune).post(jobs::phone_calls_prune),
        )
        .route(
            "/api/jobs/phone-calls-repair-links",
            get(jobs::phone_calls_repair_links).post(jobs::phone_calls_repair_links),
        )
        .route("/api/jobs/google-health", get(jobs::google_health).post(jobs::google_health))
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/version", get(ui::version))
        .fallback(ui::fallback)
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("cubic rust server listening on http://{addr}");
    if !state.cfg.ui_origin.is_empty() {
        tracing::info!("proxying UI to {}", state.cfg.ui_origin);
    } else if let Some(dir) = &state.cfg.static_dir {
        tracing::info!("serving static UI from {} (build {})", dir.display(), state.app_version);
    }
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    axum::serve(listener, app).await.expect("serve");
}
