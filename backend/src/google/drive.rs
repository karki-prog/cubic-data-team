use serde::Deserialize;
use serde_json::json;

use super::token::GoogleClient;

#[derive(Clone, Debug, Deserialize)]
pub struct DriveFile {
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "mimeType")]
    pub mime_type: Option<String>,
    #[serde(rename = "webViewLink")]
    pub web_view_link: Option<String>,
    #[serde(rename = "modifiedTime")]
    pub modified_time: Option<String>,
    pub capabilities: Option<DriveCaps>,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct DriveCaps {
    #[serde(rename = "canEdit")]
    pub can_edit: Option<bool>,
    #[serde(rename = "canAddChildren")]
    pub can_add_children: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DriveWatchChannel {
    pub id: Option<String>,
    #[serde(rename = "resourceId")]
    pub resource_id: Option<String>,
    pub expiration: Option<String>,
}

#[derive(Deserialize)]
struct ListResponse {
    files: Option<Vec<DriveFile>>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

impl GoogleClient {
    pub async fn drive_get(&self, file_id: &str, fields: &str, user: bool) -> anyhow::Result<DriveFile> {
        let token = if user {
            self.user_token("calendar").await?
        } else {
            self.sa_token().await?
        };
        let url = format!(
            "https://www.googleapis.com/drive/v3/files/{}?fields={}&supportsAllDrives=true",
            urlencoding::encode(file_id),
            urlencoding::encode(fields)
        );
        let res = self.http().get(url).bearer_auth(token).send().await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            anyhow::bail!("drive files.get failed: {body}");
        }
        Ok(serde_json::from_str(&body)?)
    }

    pub async fn drive_list(
        &self,
        q: &str,
        fields: &str,
        page_size: u32,
        page_token: Option<&str>,
        order_by: Option<&str>,
    ) -> anyhow::Result<(Vec<DriveFile>, Option<String>)> {
        let token = self.sa_token().await?;
        let mut url = format!(
            "https://www.googleapis.com/drive/v3/files?q={}&fields={}&pageSize={page_size}&supportsAllDrives=true&includeItemsFromAllDrives=true",
            urlencoding::encode(q),
            urlencoding::encode(fields)
        );
        if let Some(token) = page_token {
            url.push_str(&format!("&pageToken={}", urlencoding::encode(token)));
        }
        if let Some(order) = order_by {
            url.push_str(&format!("&orderBy={}", urlencoding::encode(order)));
        }
        let res = self.http().get(url).bearer_auth(token).send().await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            anyhow::bail!("drive files.list failed: {body}");
        }
        let parsed: ListResponse = serde_json::from_str(&body)?;
        Ok((parsed.files.unwrap_or_default(), parsed.next_page_token))
    }

    pub async fn drive_list_all(&self, q: &str, fields: &str) -> anyhow::Result<Vec<DriveFile>> {
        let mut out = Vec::new();
        let mut page = None;
        loop {
            let (files, next) = self.drive_list(q, fields, 200, page.as_deref(), None).await?;
            out.extend(files);
            match next {
                Some(t) if !t.is_empty() => page = Some(t),
                _ => break,
            }
        }
        Ok(out)
    }

    pub async fn find_child_folder(&self, parent_id: &str, name: &str) -> anyhow::Result<Option<String>> {
        let escaped = name.replace('\'', r"\'");
        let q = format!(
            "'{parent_id}' in parents and mimeType='application/vnd.google-apps.folder' and trashed=false and name='{escaped}'"
        );
        let (files, _) = self
            .drive_list(&q, "files(id,name)", 5, None, None)
            .await?;
        Ok(files.into_iter().next().and_then(|f| f.id))
    }

    async fn create_folder_user(&self, parent_id: &str, name: &str) -> anyhow::Result<String> {
        let token = self.user_token("calendar").await?;
        let url = "https://www.googleapis.com/drive/v3/files?fields=id&supportsAllDrives=true";
        let res = self
            .http()
            .post(url)
            .bearer_auth(token)
            .json(&json!({
                "name": name,
                "mimeType": "application/vnd.google-apps.folder",
                "parents": [parent_id]
            }))
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            anyhow::bail!("drive folder create failed: {body}");
        }
        let parsed: DriveFile = serde_json::from_str(&body)?;
        parsed
            .id
            .ok_or_else(|| anyhow::anyhow!("Failed to create Drive folder \"{name}\"."))
    }

    pub async fn find_or_create_folder(&self, parent_id: &str, name: &str) -> anyhow::Result<String> {
        if let Some(id) = self.find_child_folder(parent_id, name).await? {
            return Ok(id);
        }
        self.create_folder_user(parent_id, name).await
    }

    pub async fn make_anyone_reader(&self, file_id: &str) {
        let Ok(token) = self.user_token("calendar").await else {
            return;
        };
        let url = format!(
            "https://www.googleapis.com/drive/v3/files/{}/permissions?supportsAllDrives=true",
            urlencoding::encode(file_id)
        );
        let res = self
            .http()
            .post(url)
            .bearer_auth(token)
            .json(&json!({ "type": "anyone", "role": "reader" }))
            .send()
            .await;
        if let Err(err) = res {
            tracing::warn!("[drive] public share skipped: {err}");
        }
    }

    pub async fn upload_bytes(
        &self,
        parent_id: &str,
        filename: &str,
        mime_type: &str,
        data: &[u8],
    ) -> anyhow::Result<(String, String, String)> {
        let token = self.user_token("calendar").await?;
        let boundary = format!("cubic_{}", uuid::Uuid::new_v4().simple());
        let metadata = serde_json::to_string(&json!({
            "name": filename,
            "parents": [parent_id]
        }))?;
        let mut body = Vec::new();
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata}\r\n--{boundary}\r\nContent-Type: {mime_type}\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(data);
        body.extend_from_slice(format!("\r\n--{boundary}--").as_bytes());

        let url = "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&fields=id,name,webViewLink&supportsAllDrives=true";
        let res = self
            .http()
            .post(url)
            .bearer_auth(token)
            .header(
                "content-type",
                format!("multipart/related; boundary={boundary}"),
            )
            .body(body)
            .send()
            .await?;
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            anyhow::bail!("drive upload failed: {text}");
        }
        let parsed: DriveFile = serde_json::from_str(&text)?;
        let id = parsed
            .id
            .ok_or_else(|| anyhow::anyhow!("Failed to upload \"{filename}\"."))?;
        self.make_anyone_reader(&id).await;
        let url = parsed
            .web_view_link
            .unwrap_or_else(|| format!("https://drive.google.com/file/d/{id}/view"));
        Ok((id, parsed.name.unwrap_or_else(|| filename.to_string()), url))
    }

    /// Ask Drive to POST cubic-data.com when this spreadsheet file changes.
    pub async fn drive_watch_file(
        &self,
        file_id: &str,
        channel_id: &str,
        address: &str,
        token: &str,
        expiration_ms: i64,
    ) -> anyhow::Result<DriveWatchChannel> {
        let sa = self.sa_token().await?;
        let url = format!(
            "https://www.googleapis.com/drive/v3/files/{}/watch?supportsAllDrives=true",
            urlencoding::encode(file_id)
        );
        let res = self
            .http()
            .post(url)
            .bearer_auth(sa)
            .json(&json!({
                "id": channel_id,
                "type": "web_hook",
                "address": address,
                "token": token,
                "expiration": expiration_ms,
            }))
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            anyhow::bail!("drive files.watch failed: {body}");
        }
        Ok(serde_json::from_str(&body)?)
    }

    pub async fn drive_stop_channel(&self, channel_id: &str, resource_id: &str) -> anyhow::Result<()> {
        let sa = self.sa_token().await?;
        let res = self
            .http()
            .post("https://www.googleapis.com/drive/v3/channels/stop")
            .bearer_auth(sa)
            .json(&json!({
                "id": channel_id,
                "resourceId": resource_id,
            }))
            .send()
            .await?;
        if !res.status().is_success() && res.status().as_u16() != 404 {
            anyhow::bail!("drive channels.stop failed: {}", res.text().await?);
        }
        Ok(())
    }

    pub async fn drive_delete(&self, file_id: &str, user: bool) -> anyhow::Result<()> {
        let token = if user {
            self.user_token("calendar").await?
        } else {
            self.sa_token().await?
        };
        let url = format!(
            "https://www.googleapis.com/drive/v3/files/{}?supportsAllDrives=true",
            urlencoding::encode(file_id)
        );
        let res = self.http().delete(url).bearer_auth(token).send().await?;
        if !res.status().is_success() && res.status().as_u16() != 404 {
            anyhow::bail!("drive delete failed: {}", res.text().await?);
        }
        Ok(())
    }
}

pub fn folder_view_url(id: &str) -> String {
    format!("https://drive.google.com/drive/folders/{id}")
}
