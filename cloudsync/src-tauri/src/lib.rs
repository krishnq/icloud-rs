use std::sync::Arc;
use tokio::sync::Mutex;
use cloudsync_core::icloud::core::client::ICloudClient;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub struct AppState(pub Arc<Mutex<Option<ICloudClient>>>);

static INIT_CALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(serde::Serialize)]
struct InitResponse {
    status: String,
    drive_path: Option<String>,
    photos_path: Option<String>,
}

#[tauri::command]
async fn init_app(state: tauri::State<'_, AppState>) -> Result<InitResponse, String> {
    let config = cloudsync_core::icloud::core::config::load_config();
    
    // For now, handle the first iCloud account found in config
    let account = config.accounts.iter().find(|(_, c)| c.account_type == "icloud");
    
    if INIT_CALLED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        if let Some((_, acc_cfg)) = account {
            return Ok(InitResponse {
                status: "RESTORED".to_string(),
                drive_path: acc_cfg.mount_drive.clone(),
                photos_path: acc_cfg.mount_photos.clone(),
            });
        }
        return Ok(InitResponse { status: "NEEDS_LOGIN".to_string(), drive_path: None, photos_path: None });
    }

    if let Some((account_name, acc_cfg)) = account {
        if let Ok(session_data) = cloudsync_core::icloud::core::client::SessionData::load_from_keyring(account_name) {
            println!("Found credentials for {} in keyring!", account_name);
            let mut client = ICloudClient::new("").map_err(|e| e.to_string())?;
            client.session_data = session_data.clone();
            
            // Re-inject cookies
            if let Err(e) = client.inject_cookies(session_data.raw_cookies.clone()) {
                eprintln!("Failed to inject saved cookies: {}", e);
            }
            
            // Test session
            if let Err(e) = client.bootstrap_account().await {
                eprintln!("Saved session is expired or invalid: {}", e);
                return Ok(InitResponse { status: "NEEDS_LOGIN".to_string(), drive_path: None, photos_path: None });
            } else {
                println!("Successfully restored session for {}", account_name);
                *state.0.lock().await = Some(client.clone());
                
                // Auto-mount FUSE directories
                if let Some(drive_path) = &acc_cfg.mount_drive {
                    let c = client.clone();
                    let p = drive_path.clone();
                    std::thread::spawn(move || {
                        println!("Auto-mounting FUSE drive at {}...", p);
                        let _ = cloudsync_core::icloud::drive::vfs::mount_drive(c, &p);
                    });
                }
                if let Some(photos_path) = &acc_cfg.mount_photos {
                    let c = client.clone();
                    let p = photos_path.clone();
                    std::thread::spawn(move || {
                        println!("Auto-mounting FUSE photos at {}...", p);
                        let _ = cloudsync_core::icloud::photos::vfs::mount_photos(c, &p);
                    });
                }
                
                return Ok(InitResponse {
                    status: "RESTORED".to_string(),
                    drive_path: acc_cfg.mount_drive.clone(),
                    photos_path: acc_cfg.mount_photos.clone(),
                });
            }
        }
    }
    Ok(InitResponse { status: "NEEDS_LOGIN".to_string(), drive_path: None, photos_path: None })
}

#[tauri::command]
async fn open_icloud_login(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<String, String> {
    // Initialize empty client
    let client = ICloudClient::new("").map_err(|e| e.to_string())?;
    *state.0.lock().await = Some(client);

    // Ensure previous window is closed if it exists
    if let Some(existing) = app.get_webview_window("icloud-login") {
        let _ = existing.close();
    }

    let window = WebviewWindowBuilder::new(
        &app,
        "icloud-login",
        WebviewUrl::External("https://www.icloud.com/".parse().unwrap()),
    )
    .title("Sign in to iCloud")
    .inner_size(800.0, 600.0)
    .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15")
    .build()
    .map_err(|e| e.to_string())?;

    // Poll for cookies
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        
        let cookies = window.cookies().unwrap_or_default();
        let mut found_auth = false;
        
        let mut cookie_vec = Vec::new();
        for cookie in cookies {
            // Log cookie names to help debug
            println!("Detected Cookie: {}", cookie.name());
            
            cookie_vec.push((cookie.name().to_string(), cookie.value().to_string()));
            // Apple suffixes the WEB_KB cookie with a random ID, so we check the prefix
            if cookie.name().starts_with("X_APPLE_WEB_KB") || cookie.name() == "X-APPLE-WEBAUTH-TOKEN" {
                found_auth = true;
            }
        }
        
        if found_auth {
            let mut state_guard = state.0.lock().await;
            if let Some(client) = state_guard.as_mut() {
                client.inject_cookies(cookie_vec).map_err(|e| e.to_string())?;
                // Test the session
                if let Err(e) = client.bootstrap_account().await {
                    eprintln!("Bootstrap failed: {}", e);
                } else {
                    println!("Bootstrap successful!");
                    // Save to keyring so we don't have to login again next time!
                    if let Err(e) = client.session_data.save_to_keyring("default_icloud") {
                        eprintln!("Failed to save session to keyring: {}", e);
                    }
                }
            }
            let _ = window.close();
            return Ok("SUCCESS".to_string());
        }

        // Break if user closed the window
        if app.get_webview_window("icloud-login").is_none() {
            return Err("User closed login window".to_string());
        }
    }
}

#[tauri::command]
async fn mount_drive(state: tauri::State<'_, AppState>, drive_path: String) -> Result<String, String> {
    let client = {
        let state_guard = state.0.lock().await;
        state_guard.as_ref().ok_or("Client not initialized")?.clone()
    };

    // Spawn a standard OS thread because fuser::mount2 blocks forever
    std::thread::spawn(move || {
        println!("Mounting FUSE drive at {}...", drive_path);
        match cloudsync_core::icloud::drive::vfs::mount_drive(client, &drive_path) {
            Ok(_) => println!("FUSE drive unmounted normally."),
            Err(e) => eprintln!("FUSE MOUNT ERROR: {}", e),
        }
    });

    Ok("SUCCESS".to_string())
}

#[tauri::command]
async fn mount_photos(state: tauri::State<'_, AppState>, photos_path: String) -> Result<String, String> {
    let client = {
        let state_guard = state.0.lock().await;
        state_guard.as_ref().ok_or("Client not initialized")?.clone()
    };

    std::thread::spawn(move || {
        println!("Mounting FUSE photos at {}...", photos_path);
        match cloudsync_core::icloud::photos::vfs::mount_photos(client, &photos_path) {
            Ok(_) => println!("FUSE photos unmounted normally."),
            Err(e) => eprintln!("FUSE PHOTOS MOUNT ERROR: {}", e),
        }
    });

    Ok("SUCCESS".to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState(Arc::new(Mutex::new(None))))
        .invoke_handler(tauri::generate_handler![init_app, open_icloud_login, mount_drive, mount_photos])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
