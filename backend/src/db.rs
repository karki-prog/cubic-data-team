//! Fast website copy of spreadsheet data (hiring / DNA / allow-list / job-meta)
//! plus the in-flight booking queue. Spreadsheets stay the source of truth.
//! JSON files under `data/` are only read once, to migrate into Mongo.

use mongodb::bson::{doc, spec::BinarySubtype, Binary, Bson, Document};
use mongodb::options::{FindOneAndUpdateOptions, ReplaceOptions, ReturnDocument, UpdateModifications};
use mongodb::{Client, Collection, Database};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

const SITE_CACHE: &str = "site_cache";
const BOOKING_QUEUE: &str = "booking_queue";

#[derive(Clone)]
pub struct Db {
    inner: Option<Database>,
}

impl Db {
    pub async fn connect(uri: &str) -> Self {
        let uri = uri.trim();
        if uri.is_empty() {
            tracing::warn!("MONGODB_URI is empty — site caches stay on disk until it is set");
            return Self { inner: None };
        }
        match connect_inner(uri).await {
            Ok(db) => {
                tracing::info!("MongoDB connected ({})", db.name());
                Self { inner: Some(db) }
            }
            Err(err) => {
                tracing::warn!(error = %err, "MongoDB unavailable — site caches stay on disk");
                Self { inner: None }
            }
        }
    }

    pub fn is_connected(&self) -> bool {
        self.inner.is_some()
    }

    fn cache(&self) -> Option<Collection<Document>> {
        self.inner.as_ref().map(|db| db.collection(SITE_CACHE))
    }

    fn queue(&self) -> Option<Collection<Document>> {
        self.inner.as_ref().map(|db| db.collection(BOOKING_QUEUE))
    }

    pub async fn get_json(&self, id: &str) -> Option<Value> {
        let coll = self.cache()?;
        let doc = coll.find_one(doc! { "_id": id }).await.ok().flatten()?;
        match doc.get("body") {
            Some(Bson::Document(d)) => mongodb::bson::from_document::<Value>(d.clone()).ok(),
            Some(other) => mongodb::bson::from_bson::<Value>(other.clone()).ok(),
            None => None,
        }
    }

    pub async fn put_json(&self, id: &str, body: &Value) -> anyhow::Result<()> {
        let Some(coll) = self.cache() else {
            anyhow::bail!("MongoDB is not connected");
        };
        let bson = mongodb::bson::to_bson(body)?;
        let replace = doc! {
            "_id": id,
            "body": bson,
            "updatedAt": chrono::Utc::now().to_rfc3339(),
        };
        coll.replace_one(doc! { "_id": id }, replace)
            .with_options(ReplaceOptions::builder().upsert(true).build())
            .await?;
        Ok(())
    }

    /// Copy a JSON file into Mongo if that key is still empty.
    pub async fn migrate_json_file(&self, id: &str, path: &Path) {
        if !self.is_connected() {
            return;
        }
        if self.get_json(id).await.is_some() {
            return;
        }
        let Ok(raw) = std::fs::read(path) else {
            return;
        };
        let Ok(body) = serde_json::from_slice::<Value>(&raw) else {
            return;
        };
        if body.is_null() {
            return;
        }
        match self.put_json(id, &body).await {
            Ok(()) => tracing::info!("migrated {id} from {} into MongoDB", path.display()),
            Err(err) => tracing::warn!(error = %err, "could not migrate {id} into MongoDB"),
        }
    }

    pub async fn enqueue_booking(
        &self,
        id: &str,
        meta: &Value,
        resume: &[u8],
    ) -> anyhow::Result<()> {
        let Some(coll) = self.queue() else {
            anyhow::bail!("MongoDB is not connected");
        };
        let mut doc = mongodb::bson::to_document(meta)?;
        doc.insert("_id", id);
        doc.insert(
            "resume",
            Binary {
                subtype: BinarySubtype::Generic,
                bytes: resume.to_vec(),
            },
        );
        coll.replace_one(doc! { "_id": id }, doc)
            .with_options(ReplaceOptions::builder().upsert(true).build())
            .await?;
        Ok(())
    }

    pub async fn list_booking_jobs(&self) -> anyhow::Result<Vec<(String, Value, Vec<u8>)>> {
        let Some(coll) = self.queue() else {
            anyhow::bail!("MongoDB is not connected");
        };
        let mut cursor = coll.find(doc! {}).await?;
        let mut out = Vec::new();
        while cursor.advance().await? {
            let d = cursor.deserialize_current()?;
            let id = d.get_str("_id").unwrap_or("").to_string();
            if id.is_empty() {
                continue;
            }
            let resume = match d.get("resume") {
                Some(Bson::Binary(b)) => b.bytes.clone(),
                _ => Vec::new(),
            };
            let mut meta = d;
            meta.remove("resume");
            meta.remove("_id");
            if let Ok(v) = mongodb::bson::from_document::<Value>(meta) {
                out.push((id, v, resume));
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    pub async fn save_booking_job(&self, id: &str, meta: &Value, resume: Option<&[u8]>) -> anyhow::Result<()> {
        let Some(coll) = self.queue() else {
            anyhow::bail!("MongoDB is not connected");
        };
        let mut set = mongodb::bson::to_document(meta)?;
        set.remove("_id");
        if let Some(bytes) = resume {
            set.insert(
                "resume",
                Binary {
                    subtype: BinarySubtype::Generic,
                    bytes: bytes.to_vec(),
                },
            );
        }
        coll.update_one(doc! { "_id": id }, doc! { "$set": set }).await?;
        Ok(())
    }

    pub async fn delete_booking_job(&self, id: &str) -> anyhow::Result<()> {
        let Some(coll) = self.queue() else {
            return Ok(());
        };
        coll.delete_one(doc! { "_id": id }).await?;
        Ok(())
    }

    pub async fn claim_booking_job(&self, id: &str) -> anyhow::Result<bool> {
        let Some(coll) = self.queue() else {
            anyhow::bail!("MongoDB is not connected");
        };
        let opts = FindOneAndUpdateOptions::builder()
            .return_document(ReturnDocument::After)
            .build();
        let found = coll
            .find_one_and_update(
                doc! { "_id": id, "status": { "$in": ["queued", "failed", "processing"] } },
                UpdateModifications::Document(doc! {
                    "$set": { "status": "processing", "updatedAt": chrono::Utc::now().to_rfc3339() },
                    "$inc": { "attempts": 1 }
                }),
            )
            .with_options(opts)
            .await?;
        Ok(found.is_some())
    }
}

async fn connect_inner(uri: &str) -> anyhow::Result<Database> {
    let mut opts = mongodb::options::ClientOptions::parse(uri).await?;
    opts.server_selection_timeout = Some(Duration::from_secs(4));
    opts.connect_timeout = Some(Duration::from_secs(4));
    let client = Client::with_options(opts)?;
    let db_name = uri
        .rsplit('/')
        .next()
        .and_then(|s| s.split('?').next())
        .filter(|s| !s.is_empty() && *s != uri)
        .unwrap_or("cubic_data");
    let db = client.database(db_name);
    db.run_command(doc! { "ping": 1 }).await?;
    Ok(db)
}
