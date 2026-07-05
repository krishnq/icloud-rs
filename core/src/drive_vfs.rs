use fuser::{FileAttr, FileType, Filesystem, ReplyAttr, ReplyDirectory, ReplyData, ReplyEntry, ReplyOpen, Request};
use libc::{ENOENT, EIO};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;
use tokio::sync::Semaphore;

use crate::drive::DriveNode;
use crate::client::ICloudClient;

const TTL: Duration = Duration::from_secs(1);

pub struct ICloudDriveFS {
    pub client: Arc<ICloudClient>,
    pub rt: Arc<Runtime>,
    pub nodes_cache: HashMap<u64, Vec<DriveNode>>,
    pub inode_to_id: HashMap<u64, String>,
    pub fetch_semaphore: Arc<Semaphore>,
    pub active_downloads: Arc<Mutex<HashMap<u64, String>>>,
    pub chunk_cache: Arc<Mutex<HashMap<u64, Vec<(u64, Vec<u8>)>>>>,
    pub next_fh: u64,
}

impl ICloudDriveFS {
    pub fn new(client: ICloudClient) -> Self {
        let mut inode_to_id = HashMap::new();
        // The root inode 1 maps to the CloudDocs root
        inode_to_id.insert(1, "FOLDER::com.apple.CloudDocs::root".to_string());
        
        Self {
            client: Arc::new(client),
            rt: Arc::new(Runtime::new().expect("Failed to create Tokio runtime for VFS")),
            nodes_cache: HashMap::new(),
            inode_to_id,
            fetch_semaphore: Arc::new(Semaphore::new(3)),
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            chunk_cache: Arc::new(Mutex::new(HashMap::new())),
            next_fh: 1,
        }
    }

    // Basic hash to generate a stable inode from iCloud ID
    fn get_inode_for_id(&self, id: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        id.hash(&mut hasher);
        // Avoid ino 1 which is root
        hasher.finish().max(2)
    }

    fn ensure_nodes_loaded(&mut self, ino: u64) {
        if self.nodes_cache.contains_key(&ino) {
            return; // Already cached
        }

        if let Some(folder_id) = self.inode_to_id.get(&ino).cloned() {
            let client_clone = Arc::clone(&self.client);
            let sem_clone = Arc::clone(&self.fetch_semaphore);
            
            let folder_id_clone = folder_id.clone();
            let nodes_result = self.rt.block_on(async move {
                let _permit = sem_clone.acquire().await.unwrap();
                client_clone.fetch_drive_folder(&folder_id_clone).await
            });
            
            match nodes_result {
                Ok(nodes) => {
                    // Update reverse lookup map for any children
                    for node in &nodes {
                        let child_ino = self.get_inode_for_id(&node.id);
                        self.inode_to_id.insert(child_ino, node.id.clone());
                    }
                    self.nodes_cache.insert(ino, nodes);
                },
                Err(e) => eprintln!("FAILED to fetch drive folder {}: {}", folder_id, e),
            }
        }
    }
    
    // Find node by inode across all cached directories
    fn find_cached_node(&self, ino: u64) -> Option<&DriveNode> {
        for nodes in self.nodes_cache.values() {
            if let Some(node) = nodes.iter().find(|n| self.get_inode_for_id(&n.id) == ino) {
                return Some(node);
            }
        }
        None
    }
}

const DIR_ATTR: FileAttr = FileAttr {
    ino: 1,
    size: 0,
    blocks: 0,
    atime: UNIX_EPOCH,
    mtime: UNIX_EPOCH,
    ctime: UNIX_EPOCH,
    crtime: UNIX_EPOCH,
    kind: FileType::Directory,
    perm: 0o755,
    nlink: 2,
    uid: 1000,
    gid: 1000,
    rdev: 0,
    flags: 0,
    blksize: 512,
};

pub fn mount_drive(client: ICloudClient, mountpoint: &str) -> Result<(), std::io::Error> {
    let vfs = ICloudDriveFS::new(client);
    let options = vec![
        fuser::MountOption::RO, // Read-only for now
        fuser::MountOption::FSName("icloud-drive".to_string()),
        fuser::MountOption::AutoUnmount,
    ];
    fuser::mount2(vfs, mountpoint, &options)
}

impl Filesystem for ICloudDriveFS {
    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        if ino == 1 {
            reply.attr(&TTL, &DIR_ATTR);
            return;
        }
        
        if let Some(node) = self.nodes_cache.values().flatten().find(|n| self.get_inode_for_id(&n.id) == ino) {
            let mut attr = DIR_ATTR;
            let now = SystemTime::now();
            attr.ino = ino;
            attr.mtime = now;
            attr.atime = now;
            attr.ctime = now;
            attr.crtime = now;
            if node.is_dir {
                attr.kind = FileType::Directory;
            } else {
                attr.kind = FileType::RegularFile;
                attr.size = node.size.unwrap_or(0);
                attr.perm = 0o644;
            }
            reply.attr(&TTL, &attr);
            return;
        }
        
        reply.error(ENOENT);
    }

    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        self.ensure_nodes_loaded(parent);
        
        if let Some(nodes) = self.nodes_cache.get(&parent) {
            if let Some(node) = nodes.iter().find(|n| Some(n.name.as_str()) == name.to_str()) {
                let mut attr = DIR_ATTR;
                let now = SystemTime::now();
                attr.ino = self.get_inode_for_id(&node.id);
                attr.mtime = now;
                attr.atime = now;
                attr.ctime = now;
                attr.crtime = now;
                
                if node.is_dir {
                    attr.kind = FileType::Directory;
                } else {
                    attr.kind = FileType::RegularFile;
                    attr.size = node.size.unwrap_or(0);
                    attr.perm = 0o644;
                }
                reply.entry(&TTL, &attr, 0);
                return;
            }
        }
        
        reply.error(ENOENT);
    }

    fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        self.ensure_nodes_loaded(ino);

        let mut entries = vec![
            (ino, FileType::Directory, ".".to_string()),
            (if ino == 1 { 1 } else { 1 }, FileType::Directory, "..".to_string()), // Simplified parent fallback
        ];

        if let Some(nodes) = self.nodes_cache.get(&ino) {
            for node in nodes {
                let kind = if node.is_dir { FileType::Directory } else { FileType::RegularFile };
                entries.push((self.get_inode_for_id(&node.id), kind, node.name.clone()));
            }
        } else {
            reply.error(ENOENT);
            return;
        }

        for (i, entry) in entries.into_iter().enumerate().skip(offset as usize) {
            if reply.add(entry.0, (i + 1) as i64, entry.1, &entry.2) {
                break;
            }
        }
        reply.ok();
    }

    fn open(&mut self, _req: &Request, ino: u64, _flags: i32, reply: ReplyOpen) {
        if let Some(node) = self.find_cached_node(ino).cloned() {
            if node.is_dir {
                reply.error(libc::EISDIR);
                return;
            }
            
            let client_clone = Arc::clone(&self.client);
            let file_id = node.docwsid.clone().unwrap_or(node.id.clone());
            let downloads_clone = Arc::clone(&self.active_downloads);
            let fh = self.next_fh;
            self.next_fh += 1;
            
            // Spawn background task to get the pre-signed URL (this takes <100ms)
            self.rt.spawn(async move {
                let download_url = match client_clone.get_file_download_url(&file_id).await {
                    Ok(url) => url,
                    Err(e) => {
                        eprintln!("FAILED to get download url for {}: {}", node.name, e);
                        reply.error(EIO);
                        return;
                    }
                };
                
                if let Ok(mut downloads) = downloads_clone.lock() {
                    downloads.insert(fh, download_url);
                }
                
                reply.opened(fh, 0);
            });
        } else {
            reply.error(ENOENT);
        }
    }

    fn read(&mut self, _req: &Request, _ino: u64, fh: u64, offset: i64, size: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyData) {
        let download_url = {
            if let Ok(downloads) = self.active_downloads.lock() {
                downloads.get(&fh).cloned()
            } else {
                None
            }
        };
        
        if let Some(url) = download_url {
            let client_clone = Arc::clone(&self.client);
            let cache_clone = Arc::clone(&self.chunk_cache);
            let offset_u64 = offset as u64;
            let size_usize = size as usize;
            
            // Wrapper to guarantee the kernel ALWAYS gets a reply
            struct SafeReply { reply: Option<ReplyData> }
            impl SafeReply {
                fn data(&mut self, data: &[u8]) { if let Some(r) = self.reply.take() { r.data(data); } }
                fn error(&mut self, err: i32) { if let Some(r) = self.reply.take() { r.error(err); } }
            }
            impl Drop for SafeReply {
                fn drop(&mut self) {
                    if let Some(r) = self.reply.take() {
                        eprintln!("CRITICAL ERROR: FUSE read dropped without a response! Sending EIO to prevent kernel hang.");
                        r.error(EIO);
                    }
                }
            }
            
            let mut safe_reply = SafeReply { reply: Some(reply) };
            
            // Check cache FIRST to satisfy micro-reads instantly
            if let Ok(cache) = cache_clone.lock() {
                if let Some(chunks) = cache.get(&fh) {
                    for chunk in chunks {
                        if offset_u64 >= chunk.0 && offset_u64 < chunk.0 + chunk.1.len() as u64 {
                            let start_idx = (offset_u64 - chunk.0) as usize;
                            let available = chunk.1.len() - start_idx;
                            let return_size = std::cmp::min(size_usize, available);
                            safe_reply.data(&chunk.1[start_idx..start_idx + return_size]);
                            return;
                        }
                    }
                }
            }
            
            self.rt.spawn(async move {
                let fetch_size = size as u64;
                let range_header = format!("bytes={}-{}", offset_u64, offset_u64 + fetch_size - 1);
                
                let req = client_clone.http_client.get(&url).header("Range", range_header);
                
                let timeout_result = tokio::time::timeout(Duration::from_secs(15), req.send()).await;
                
                match timeout_result {
                    Ok(Ok(res)) => {
                        let status = res.status();
                        
                        if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
                            safe_reply.data(&[]);
                            return;
                        }
                        
                        if status.is_success() || status == reqwest::StatusCode::PARTIAL_CONTENT {
                            match tokio::time::timeout(Duration::from_secs(15), res.bytes()).await {
                                Ok(Ok(bytes)) => {
                                    if status == reqwest::StatusCode::PARTIAL_CONTENT {
                                        if let Ok(mut cache) = cache_clone.lock() {
                                            cache.entry(fh).or_insert_with(Vec::new).push((offset_u64, bytes.to_vec()));
                                        }
                                        let end = std::cmp::min(size_usize, bytes.len());
                                        safe_reply.data(&bytes[..end]);
                                    } else {
                                        if let Ok(mut cache) = cache_clone.lock() {
                                            cache.entry(fh).or_insert_with(Vec::new).push((0, bytes.to_vec()));
                                        }
                                        if offset_u64 >= bytes.len() as u64 {
                                            safe_reply.data(&[]);
                                        } else {
                                            let start_idx = offset_u64 as usize;
                                            let end_idx = std::cmp::min(start_idx + size_usize, bytes.len());
                                            safe_reply.data(&bytes[start_idx..end_idx]);
                                        }
                                    }
                                },
                                _ => safe_reply.error(EIO)
                            }
                        } else {
                            safe_reply.error(EIO);
                        }
                    },
                    _ => safe_reply.error(EIO)
                }
            });
        } else {
            reply.error(EIO);
        }
    }
    
    fn release(&mut self, _req: &Request, _ino: u64, fh: u64, _flags: i32, _lock_owner: Option<u64>, _flush: bool, reply: fuser::ReplyEmpty) {
        if let Ok(mut downloads) = self.active_downloads.lock() {
            downloads.remove(&fh);
        }
        if let Ok(mut cache) = self.chunk_cache.lock() {
            cache.remove(&fh);
        }
        reply.ok();
    }
}
