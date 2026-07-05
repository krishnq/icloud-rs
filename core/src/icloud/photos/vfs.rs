use fuser::{FileAttr, FileType, Filesystem, ReplyAttr, ReplyDirectory, ReplyEntry, ReplyOpen, ReplyData, Request};
use libc::{ENOENT, EIO};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;
use tokio::sync::Semaphore;

use crate::icloud::photos::api::{PhotoAlbum, PhotoAsset};
use crate::icloud::core::client::ICloudClient;

const TTL: Duration = Duration::from_secs(1);

pub struct ICloudPhotosFS {
    pub client: Arc<ICloudClient>,
    pub rt: Arc<Runtime>,
    pub albums: Option<Vec<PhotoAlbum>>,
    pub album_photos: HashMap<u64, Vec<PhotoAsset>>,
    pub album_inode_map: HashMap<u64, String>, // ino -> album_id
    pub chunk_cache: Arc<std::sync::Mutex<HashMap<u64, Vec<(u64, Vec<u8>)>>>>, // fh -> list of (start_offset, data)
    pub fetch_semaphore: Arc<Semaphore>,
    pub next_fh: u64,
}

impl ICloudPhotosFS {
    pub fn new(client: ICloudClient) -> Self {
        Self {
            client: Arc::new(client),
            rt: Arc::new(Runtime::new().expect("Failed to create Tokio runtime for Photos VFS")),
            albums: None,
            album_photos: HashMap::new(),
            album_inode_map: HashMap::new(),
            chunk_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
            fetch_semaphore: Arc::new(Semaphore::new(3)),
            next_fh: 1,
        }
    }

    fn get_inode_for_id(&self, id: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        id.hash(&mut hasher);
        hasher.finish().max(2)
    }

    fn ensure_albums_loaded(&mut self) {
        if self.albums.is_none() {
            let client_clone = Arc::clone(&self.client);
            let sem_clone = Arc::clone(&self.fetch_semaphore);
            
            let result = self.rt.block_on(async move {
                let _permit = sem_clone.acquire().await.unwrap();
                client_clone.fetch_photo_albums().await
            });
            
            if let Ok(albums) = result {
                for album in &albums {
                    let ino = self.get_inode_for_id(&album.id);
                    self.album_inode_map.insert(ino, album.id.clone());
                }
                self.albums = Some(albums);
            }
        }
    }

    fn ensure_album_photos_loaded(&mut self, ino: u64) {
        if self.album_photos.contains_key(&ino) {
            return;
        }

        if let Some(album_id) = self.album_inode_map.get(&ino).cloned() {
            let client_clone = Arc::clone(&self.client);
            let sem_clone = Arc::clone(&self.fetch_semaphore);
            
            let album_id_clone = album_id.clone();
            let result = self.rt.block_on(async move {
                let _permit = sem_clone.acquire().await.unwrap();
                client_clone.fetch_album_photos(&album_id_clone).await
            });
            
            match result {
                Ok(photos) => {
                    self.album_photos.insert(ino, photos);
                },
                Err(e) => eprintln!("FAILED to fetch album photos {}: {}", album_id, e),
            }
        }
    }
    
    fn find_cached_photo(&self, ino: u64) -> Option<&PhotoAsset> {
        for photos in self.album_photos.values() {
            if let Some(photo) = photos.iter().find(|p| self.get_inode_for_id(&p.id) == ino) {
                return Some(photo);
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

pub fn mount_photos(client: ICloudClient, mountpoint: &str) -> Result<(), std::io::Error> {
    let vfs = ICloudPhotosFS::new(client);
    let options = vec![
        fuser::MountOption::RO,
        fuser::MountOption::FSName("icloud-photos".to_string()),
        fuser::MountOption::AutoUnmount,
    ];
    fuser::mount2(vfs, mountpoint, &options)
}

impl Filesystem for ICloudPhotosFS {
    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        if ino == 1 {
            let mut attr = DIR_ATTR;
            attr.ino = 1;
            reply.attr(&TTL, &attr);
            return;
        }
        
        if let Some(albums) = &self.albums {
            if albums.iter().any(|a| self.get_inode_for_id(&a.id) == ino) {
                let mut attr = DIR_ATTR;
                let now = SystemTime::now();
                attr.ino = ino;
                attr.mtime = now;
                attr.atime = now;
                attr.ctime = now;
                attr.crtime = now;
                reply.attr(&TTL, &attr);
                return;
            }
        }
        
        if let Some(photo) = self.find_cached_photo(ino) {
            let mut attr = DIR_ATTR;
            let now = SystemTime::now();
            attr.ino = ino;
            attr.kind = FileType::RegularFile;
            attr.size = photo.size;
            attr.perm = 0o644;
            attr.mtime = now;
            attr.atime = now;
            attr.ctime = now;
            attr.crtime = now;
            reply.attr(&TTL, &attr);
            return;
        }
        
        reply.error(ENOENT);
    }

    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        if parent == 1 {
            self.ensure_albums_loaded();
            if let Some(albums) = &self.albums {
                if let Some(album) = albums.iter().find(|a| Some(a.name.as_str()) == name.to_str()) {
                    let mut attr = DIR_ATTR;
                    attr.ino = self.get_inode_for_id(&album.id);
                    reply.entry(&TTL, &attr, 0);
                    return;
                }
            }
        } else {
            self.ensure_album_photos_loaded(parent);
            if let Some(photos) = self.album_photos.get(&parent) {
                if let Some(photo) = photos.iter().find(|p| Some(p.name.as_str()) == name.to_str()) {
                    let mut attr = DIR_ATTR;
                    let now = SystemTime::now();
                    attr.ino = self.get_inode_for_id(&photo.id);
                    attr.kind = FileType::RegularFile;
                    attr.size = photo.size;
                    attr.perm = 0o644;
                    attr.mtime = now;
                    attr.atime = now;
                    attr.ctime = now;
                    attr.crtime = now;
                    reply.entry(&TTL, &attr, 0);
                    return;
                }
            }
        }
        
        reply.error(ENOENT);
    }

    fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        let mut entries = vec![
            (ino, FileType::Directory, ".".to_string()),
            (1, FileType::Directory, "..".to_string()),
        ];

        if ino == 1 {
            self.ensure_albums_loaded();
            if let Some(albums) = &self.albums {
                for album in albums {
                    entries.push((self.get_inode_for_id(&album.id), FileType::Directory, album.name.clone()));
                }
            }
        } else {
            self.ensure_album_photos_loaded(ino);
            if let Some(photos) = self.album_photos.get(&ino) {
                for photo in photos {
                    entries.push((self.get_inode_for_id(&photo.id), FileType::RegularFile, photo.name.clone()));
                }
            } else {
                reply.error(ENOENT);
                return;
            }
        }

        for (i, entry) in entries.into_iter().enumerate().skip(offset as usize) {
            if reply.add(entry.0, (i + 1) as i64, entry.1, &entry.2) {
                break;
            }
        }
        reply.ok();
    }

    fn open(&mut self, _req: &Request, ino: u64, _flags: i32, reply: ReplyOpen) {
        if let Some(photo) = self.find_cached_photo(ino).cloned() {
            let fh = self.next_fh;
            self.next_fh += 1;
            reply.opened(fh, 0);

            // Pre-fetch typical EXIF metadata locations (first & last 64KB)
            let cache_clone = Arc::clone(&self.chunk_cache);
            let client_clone = Arc::clone(&self.client);
            let size = photo.size;
            let download_url = photo.download_url.clone();
            
            self.rt.spawn(async move {
                let chunk_size = 65536;
                let fetch_ranges = if size <= chunk_size * 2 {
                    // Small file, fetch everything
                    vec![(0, size)]
                } else {
                    vec![
                        (0, chunk_size),
                        (size - chunk_size, chunk_size)
                    ]
                };

                for (offset, len) in fetch_ranges {
                    if len == 0 { continue; }
                    let range_header = format!("bytes={}-{}", offset, offset + len - 1);
                    if let Ok(res) = client_clone.http_client.get(&download_url).header("Range", &range_header).send().await {
                        if res.status().is_success() || res.status() == reqwest::StatusCode::PARTIAL_CONTENT {
                            if let Ok(bytes) = res.bytes().await {
                                if let Ok(mut cache) = cache_clone.lock() {
                                    cache.entry(fh).or_insert_with(Vec::new).push((offset, bytes.to_vec()));
                                }
                            }
                        }
                    }
                }
            });
        } else {
            reply.error(ENOENT);
        }
    }

    fn read(&mut self, _req: &Request, ino: u64, fh: u64, offset: i64, size: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyData) {
        if let Some(photo) = self.find_cached_photo(ino).cloned() {
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
                        eprintln!("CRITICAL ERROR: FUSE read dropped without a response! Sending EIO.");
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
                            
                            // FUSE kernel read-ahead treats short reads as EOF!
                            // Only return cached data if we can satisfy the ENTIRE request
                            // OR if we are truly at the end of the file.
                            if available >= size_usize || (chunk.0 + chunk.1.len() as u64) >= photo.size {
                                let return_size = std::cmp::min(size_usize, available);
                                safe_reply.data(&chunk.1[start_idx..start_idx + return_size]);
                                return;
                            }
                        }
                    }
                }
            }
            
            self.rt.spawn(async move {
                // Let the Linux kernel handle read-ahead; fetch exactly what FUSE asks for.
                let fetch_size = size as u64;
                let range_header = format!("bytes={}-{}", offset_u64, offset_u64 + fetch_size - 1);
                
                let req = client_clone.http_client.get(&photo.download_url).header("Range", &range_header);
                
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
                                        // CDN ignored Range, returned full file!
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
            reply.error(ENOENT);
        }
    }
    
    fn release(&mut self, _req: &Request, _ino: u64, fh: u64, _flags: i32, _lock_owner: Option<u64>, _flush: bool, reply: fuser::ReplyEmpty) {
        if let Ok(mut cache) = self.chunk_cache.lock() {
            cache.remove(&fh);
        }
        reply.ok();
    }
}
