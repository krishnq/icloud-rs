use std::sync::Arc;
use reqwest::{Client, ClientBuilder};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct ICloudClient {
    pub(crate) http_client: Client,
    pub cookie_jar: Arc<reqwest::cookie::Jar>,
    pub session_data: SessionData,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct SessionData {
    pub apple_id: String,
    pub client_id: String,
    pub session_token: Option<String>,
    pub trust_token: Option<String>,
    pub ds_web_auth_token: Option<String>,
    pub scnt: Option<String>,
    pub session_id: Option<String>,
    pub drivews_url: Option<String>,
    pub docws_url: Option<String>,
    pub ckdatabasews_url: Option<String>,
    // Store raw cookies so we can recreate the Jar when loading from keyring
    pub raw_cookies: Vec<(String, String)>,
}

impl SessionData {
    pub fn save_to_keyring(&self, account_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let entry = keyring::Entry::new("icloud-rs", account_name)?;
        let json = serde_json::to_string(self)?;
        entry.set_password(&json)?;
        Ok(())
    }

    pub fn load_from_keyring(account_name: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let entry = keyring::Entry::new("icloud-rs", account_name)?;
        let json = entry.get_password()?;
        let data = serde_json::from_str(&json)?;
        Ok(data)
    }
}

impl ICloudClient {
    pub fn new(apple_id: &str) -> Result<Self, reqwest::Error> {
        let cookie_jar = Arc::new(reqwest::cookie::Jar::default());
        
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ORIGIN,
            reqwest::header::HeaderValue::from_static("https://www.icloud.com"),
        );
        headers.insert(
            reqwest::header::REFERER,
            reqwest::header::HeaderValue::from_static("https://www.icloud.com/"),
        );

        let http_client = ClientBuilder::new()
            .cookie_provider(Arc::clone(&cookie_jar))
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            // Apple strictly checks User-Agents. Use a standard macOS Safari string.
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15")
            .build()?;

        Ok(Self {
            http_client,
            cookie_jar,
            session_data: SessionData {
                apple_id: apple_id.to_string(),
                client_id: "F84E07B5-8F32-4740-9749-00B26E543C12".to_string(),
                ..Default::default()
            },
        })
    }
}
