import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import "./App.css";

function App() {
  const [view, setView] = useState<"LOGIN" | "DASHBOARD" | "MOUNTED">("LOGIN");
  
  const [photosPath, setPhotosPath] = useState<string | null>(null);
  const [drivePath, setDrivePath] = useState<string | null>(null);
  
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    async function checkRestore() {
      try {
        const res = await invoke<{ status: string, drive_path: string | null, photos_path: string | null }>("init_app");
        if (res.status === "RESTORED") {
          if (res.drive_path) setDrivePath(res.drive_path);
          if (res.photos_path) setPhotosPath(res.photos_path);
          setView("MOUNTED");
        }
      } catch (err) {
        console.error("Init app failed:", err);
      } finally {
        setLoading(false);
      }
    }
    checkRestore();
  }, []);

  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setError("");
    
    try {
      await invoke<string>("open_icloud_login");
      setView("DASHBOARD");
    } catch (err) {
      setError(err as string);
    } finally {
      setLoading(false);
    }
  };

  const selectPhotosDir = async () => {
    const selected = await open({ directory: true });
    if (selected) setPhotosPath(selected as string);
  };

  const selectDriveDir = async () => {
    const selected = await open({ directory: true });
    if (selected) setDrivePath(selected as string);
  };

  const handleStartSync = async () => {
    setLoading(true);
    setError("");
    
    try {
      if (drivePath) {
        await invoke("mount_drive", { drivePath });
      }
      if (photosPath) {
        await invoke("mount_photos", { photosPath });
      }
      
      if (drivePath || photosPath) {
        setView("MOUNTED");
      } else {
        setError("Please select at least one path.");
      }
    } catch (err) {
      setError(err as string);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="container">
      <div className="card">
        {view === "LOGIN" && (
          <form onSubmit={handleLogin}>
            <div className="title">CloudSync</div>
            <p style={{ fontSize: "14px", color: "gray", marginBottom: "24px" }}>
              Sync your iCloud Photos and Drive securely.
            </p>
            <button type="submit" className="primary" disabled={loading}>
              {loading ? "Waiting for Login..." : "Sign in with iCloud"}
            </button>
            {error && <div className="error">{error}</div>}
          </form>
        )}

        {view === "DASHBOARD" && (
          <div>
            <div className="title">CloudSync</div>
            <p style={{ fontSize: "14px", color: "gray", marginBottom: "24px" }}>
              Select where to sync your data.
            </p>
            
            <button onClick={selectPhotosDir} className="secondary">
              {photosPath ? `Photos: ...${photosPath.slice(-15)}` : "Select Location for Photos"}
            </button>
            
            <button onClick={selectDriveDir} className="secondary">
              {drivePath ? `Drive: ...${drivePath.slice(-15)}` : "Select Location for Drive"}
            </button>

            {(photosPath || drivePath) && (
              <button onClick={handleStartSync} className="primary" style={{ marginTop: "24px" }} disabled={loading}>
                {loading ? "Mounting..." : "Start Sync"}
              </button>
            )}
            {error && <div className="error">{error}</div>}
          </div>
        )}

        {view === "MOUNTED" && (
          <div style={{ textAlign: "center" }}>
            <div className="title">Cloud Mounted!</div>
            <p style={{ fontSize: "14px", color: "gray", marginBottom: "24px" }}>
              Your iCloud data is now mounted at:<br />
              {drivePath && <span><strong>Drive:</strong> {drivePath}<br /></span>}
              {photosPath && <span><strong>Photos:</strong> {photosPath}<br /></span>}
            </p>
            <p style={{ fontSize: "12px", color: "gray" }}>
              Open your system file explorer to view your files natively.
            </p>
          </div>
        )}
      </div>
    </div>
  );
}

export default App;
