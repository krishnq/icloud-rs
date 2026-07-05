use crate::icloud::core::client::ICloudClient;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhotoAlbum {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhotoAsset {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub download_url: String,
}

impl ICloudClient {
    pub async fn fetch_photo_albums(&self) -> Result<Vec<PhotoAlbum>, Box<dyn std::error::Error>> {
        // For Phase 3, we simply hardcode the "All Photos" smart folder
        Ok(vec![
            PhotoAlbum {
                id: "all_photos".to_string(),
                name: "All Photos".to_string(),
            }
        ])
    }

    pub async fn fetch_album_photos(&self, _album_id: &str) -> Result<Vec<PhotoAsset>, Box<dyn std::error::Error>> {
        println!("Fetching photos from CloudKit (limit 200)...");
        
        let ck_url = self.session_data.ckdatabasews_url.as_ref()
            .ok_or("CloudKit WebService URL not found. Did you bootstrap?")?;
            
        let endpoint = format!("{}/database/1/com.apple.photos.cloud/production/private/records/query", ck_url);
        
        let url = reqwest::Url::parse_with_params(&endpoint, &[
            ("clientId", self.session_data.client_id.as_str()),
            ("remapEnums", "True"),
            ("getCurrentSyncToken", "True")
        ])?;
        
        let req_body = serde_json::json!({
            "query": {
                "recordType": "CPLAssetAndMasterByAddedDate",
                "filterBy": [
                    {
                        "fieldName": "startRank",
                        "fieldValue": { "type": "INT64", "value": 0 },
                        "comparator": "EQUALS"
                    },
                    {
                        "fieldName": "direction",
                        "fieldValue": { "type": "STRING", "value": "ASCENDING" },
                        "comparator": "EQUALS"
                    }
                ]
            },
            "resultsLimit": 200,
            "desiredKeys": [
                "filenameEnc",
                "resOriginalRes",
                "resJPEGMedRes",
                "resJPEGThumbRes"
            ],
            "zoneID": { "zoneName": "PrimarySync" }
        });
        
        let res = self.http_client.post(url)
            .json(&req_body)
            .send()
            .await?;
            
        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(format!("Photos API failed: {} - {}", status, text).into());
        }
        
        let json_data: serde_json::Value = res.json().await?;
        let mut photos = Vec::new();
        
        if let Some(records) = json_data.get("records").and_then(|r| r.as_array()) {
            for rec in records {
                let rec_type = rec.get("recordType").and_then(|r| r.as_str()).unwrap_or("");
                if rec_type != "CPLMaster" {
                    continue;
                }
                
                let id = rec.get("recordName").and_then(|r| r.as_str()).unwrap_or("").to_string();
                
                let fields = match rec.get("fields") {
                    Some(f) => f,
                    None => continue,
                };
                
                let name = if let Some(enc) = fields.get("filenameEnc").and_then(|f| f.get("value")).and_then(|v| v.as_str()) {
                    use base64::{Engine as _, engine::general_purpose};
                    match general_purpose::STANDARD.decode(enc) {
                        Ok(bytes) => String::from_utf8(bytes).unwrap_or_else(|_| "Unknown.jpg".to_string()),
                        Err(_) => "Unknown.jpg".to_string(),
                    }
                } else {
                    "Unknown.jpg".to_string()
                };
                
                let (size, download_url) = if let Some(res_val) = fields.get("resJPEGMedRes").and_then(|f| f.get("value")) {
                    // Prefer Medium Quality JPEG (~150KB) so the file explorer can quickly generate thumbnails
                    // without downloading the full 15MB HEIC!
                    let sz = res_val.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                    let url = res_val.get("downloadURL").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    (sz, url)
                } else if let Some(res_val) = fields.get("resOriginalRes").and_then(|f| f.get("value")) {
                    // Fallback to original if medium isn't available
                    let sz = res_val.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                    let url = res_val.get("downloadURL").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    (sz, url)
                } else {
                    (0, "".to_string())
                };
                
                if !download_url.is_empty() {
                    photos.push(PhotoAsset { id, name, size, download_url });
                }
            }
        }
        
        println!("Fetched {} photos from CloudKit.", photos.len());
        Ok(photos)
    }
}
