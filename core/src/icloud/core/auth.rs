use crate::icloud::core::client::ICloudClient;

impl ICloudClient {
    /// Injects raw cookies harvested from a webview into the reqwest cookie jar.
    /// This bypasses the need for the Rust client to perform the login flow natively.
    pub fn inject_cookies(&mut self, cookies: Vec<(String, String)>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = "https://www.icloud.com".parse::<reqwest::Url>()?;
        let setup_url = "https://setup.icloud.com".parse::<reqwest::Url>()?;
        
        self.session_data.raw_cookies = cookies.clone();
        
        for (name, value) in cookies {
            // Force the cookie to be valid for all subdomains of icloud.com (e.g. p56-drivews.icloud.com)
            let cookie_str = format!("{}={}; Domain=.icloud.com; Path=/", name, value);
            self.cookie_jar.add_cookie_str(&cookie_str, &url);
            self.cookie_jar.add_cookie_str(&cookie_str, &setup_url);

            // Also map key tokens directly to SessionData for easy access
            if name.starts_with("X_APPLE_WEB_KB") {
                self.session_data.session_token = Some(value.clone());
            } else if name == "X-APPLE-WEBAUTH-HSA-TRUST" {
                self.session_data.trust_token = Some(value.clone());
            }
        }
        
        Ok(())
    }

    pub async fn bootstrap_account(&mut self) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let setup_url = "https://setup.icloud.com/setup/ws/1/validate";
        
        let res = self.http_client.post(setup_url)
            .body("null")
            .send()
            .await?;
            
        if !res.status().is_success() {
            return Err(format!("Account bootstrap failed with status {}. Your cookies might be invalid or expired.", res.status()).into());
        }
            
        let data: serde_json::Value = res.json().await?;
        
        // Extract and save webservices URLs
        if let Some(webservices) = data.get("webservices") {
            if let Some(drivews) = webservices.get("drivews") {
                if let Some(url) = drivews.get("url").and_then(|u| u.as_str()) {
                    self.session_data.drivews_url = Some(url.to_string());
                }
            }
            if let Some(docws) = webservices.get("docws") {
                if let Some(url) = docws.get("url").and_then(|u| u.as_str()) {
                    self.session_data.docws_url = Some(url.to_string());
                }
            }
            if let Some(ckdatabasews) = webservices.get("ckdatabasews") {
                if let Some(url) = ckdatabasews.get("url").and_then(|u| u.as_str()) {
                    self.session_data.ckdatabasews_url = Some(url.to_string());
                }
            }
        }
        
        Ok(data)
    }
}
