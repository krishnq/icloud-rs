use crate::icloud::core::client::ICloudClient;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DriveNode {
    pub id: String,
    pub docwsid: Option<String>,
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

#[derive(Serialize)]
#[allow(non_snake_case)]
struct DriveRequestItem {
    drivewsid: String,
    partialData: bool,
}

impl ICloudClient {
    /// Fetch a specific directory of iCloud Drive
    pub async fn fetch_drive_folder(&self, folder_id: &str) -> Result<Vec<DriveNode>, Box<dyn std::error::Error>> {
        println!("Fetching real drive folder metadata from Apple (ID: {})...", folder_id);
        
        let drivews_url = self.session_data.drivews_url.as_ref()
            .ok_or("Drive WebService URL not found. Did you bootstrap?")?;
            
        let endpoint = format!("{}/retrieveItemDetailsInFolders", drivews_url);
        
        let req_body = vec![DriveRequestItem {
            drivewsid: folder_id.to_string(),
            partialData: false,
        }];
        
        // Use client_id as query param
        let url = reqwest::Url::parse_with_params(&endpoint, &[("clientId", &self.session_data.client_id)])?;
        
        let res = self.http_client.post(url)
            .json(&req_body)
            .send()
            .await?;
            
        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(format!("Drive API failed: {} - {}", status, text).into());
        }
        
        let json_data: serde_json::Value = res.json().await?;
        
        let mut nodes = Vec::new();
        
        // Parse the first array element (which contains our requested root folder)
        if let Some(root_obj) = json_data.as_array().and_then(|arr| arr.get(0)) {
            // Children are in the "items" array
            if let Some(items) = root_obj.get("items").and_then(|i| i.as_array()) {
                for item in items {
                    let id = item.get("drivewsid").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
                    
                    let extension = item.get("extension").and_then(|v| v.as_str());
                    let full_name = match extension {
                        Some(ext) if !ext.is_empty() => format!("{}.{}", name, ext),
                        _ => name,
                    };
                    
                    let type_str = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    let is_dir = type_str.eq_ignore_ascii_case("folder");
                    
                    let size = item.get("size").and_then(|v| v.as_u64());
                    
                    let docwsid = item.get("docwsid").and_then(|v| v.as_str()).map(|s| s.to_string());
                    
                    nodes.push(DriveNode {
                        id,
                        docwsid,
                        name: full_name,
                        is_dir,
                        size,
                    });
                }
            }
        }
        
        Ok(nodes)
    }

    /// Fetch the download URL for a specific file
    pub async fn get_file_download_url(&self, document_id: &str) -> Result<String, Box<dyn std::error::Error>> {
        let docws_url = self.session_data.docws_url.as_ref()
            .ok_or("Doc WebService URL not found. Did you bootstrap?")?;
            
        let endpoint = format!("{}/ws/com.apple.CloudDocs/download/by_id", docws_url);
        
        let url = reqwest::Url::parse_with_params(&endpoint, &[
            ("clientId", self.session_data.client_id.as_str()),
            ("document_id", document_id)
        ])?;
        
        let res = self.http_client.get(url).send().await?;
            
        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(format!("Download API failed: {} - {}", status, text).into());
        }
        
        let json_data: serde_json::Value = res.json().await?;
        
        if let Some(data_token) = json_data.get("data_token") {
            if let Some(url) = data_token.get("url").and_then(|u| u.as_str()) {
                return Ok(url.to_string());
            }
        }
        
        if let Some(package_token) = json_data.get("package_token") {
            if let Some(url) = package_token.get("url").and_then(|u| u.as_str()) {
                return Ok(url.to_string());
            }
        }
        
        Err("No valid download URL found in response".into())
    }
}
