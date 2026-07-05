use crate::client::ICloudClient;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use futures_util::StreamExt; // For streaming the response body

impl ICloudClient {
    pub async fn download_file_chunked(&self, download_url: &str, target_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let res = self.http_client.get(download_url).send().await?;
        
        if !res.status().is_success() {
            return Err(format!("Failed to download: HTTP {}", res.status()).into());
        }

        // Open a file handle via Tokio's async FS
        let mut file = File::create(target_path).await?;
        
        // Stream the bytes directly to disk without loading the whole file into RAM
        let mut stream = res.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
        }

        Ok(())
    }
}
