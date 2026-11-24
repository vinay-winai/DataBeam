use crate::backend::{croc_receive_working, parse_croc_direction, CrocReceiveOptions, CrocRoute, ProcessHandle, TransferMsg};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ReceiveMode {
    #[default]
    Croc,
    Sendme,
    EasySendMe,
}

impl ReceiveMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Croc => "Croc",
            Self::Sendme => "Sendme",
            Self::EasySendMe => "EasySendMe",
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct ReceiveEntry {
    pub id: String,
    #[serde(default)]
    pub mode: ReceiveMode,
    #[serde(default)]
    pub ticket: Option<String>,
    #[serde(default)]
    pub blob_root: Option<PathBuf>,
    pub destination: PathBuf,
    pub original_code: Option<String>,
    pub code: Option<String>,
    pub wrapper: Option<PathBuf>,
    pub done: u64,
    pub total: u64,
    pub complete: bool,
    pub ready_to_save: bool,
    #[serde(default)]
    unwrap_on_save: bool,
    pub updated: u64,
    #[serde(default)]
    pub completed_at: Option<u64>,
    #[serde(default)]
    pub elapsed_ms: Option<u64>,
    #[serde(default)]
    pub route: Option<CrocRoute>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    pub error: String,
}

#[derive(Deserialize)]
struct Offer {
    wrapper: String,
    total: u64,
    strong: bool,
}

#[derive(Deserialize)]
pub struct BatchReview {
    pub id: String,
    pub replaced: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

pub fn pending_review(history: &Path, entry_id: &str) -> Option<BatchReview> {
    if !valid_id(entry_id) {
        return None;
    }
    let input = fs::File::open(history.join(format!("{entry_id}.offer.review"))).ok()?;
    let review: BatchReview = serde_json::from_reader(input).ok()?;
    if !valid_id(&review.id) {
        return None;
    }
    let answer = fs::read_to_string(history.join(format!("{entry_id}.offer.decision"))).ok();
    if answer.as_deref().and_then(|answer| answer.split_once(' ')).is_some_and(|(_, id)| id == review.id) {
        return None;
    }
    Some(review)
}

pub fn answer_review(history: &Path, entry_id: &str, review_id: &str, apply: bool) -> io::Result<()> {
    if !pending_review(history, entry_id).is_some_and(|review| review.id == review_id) {
        return Err(io::Error::other("The file review has ended. Open Receive history to resume again."));
    }
    let mut file = tempfile::NamedTempFile::new_in(history)?;
    write!(file, "{} {review_id}", if apply { "apply" } else { "keep" })?;
    file.as_file().sync_all()?;
    file.persist(history.join(format!("{entry_id}.offer.decision"))).map_err(|error| error.error)?;
    Ok(())
}

fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

impl ReceiveEntry {
    pub fn native_cache(&self) -> Option<PathBuf> {
        let hash = crate::backend::native_ticket_to_hex_hash(self.ticket.as_deref()?)?;
        Some(self.blob_root.as_ref()?.join(format!(".sendme-recv-{hash}")))
    }

    fn work(&self) -> PathBuf {
        self.destination.join(format!(".databeam-croc-{}", self.id))
    }
    fn offer(&self, history: &Path) -> PathBuf {
        history.join(format!("{}.offer", self.id))
    }
    fn receipt(&self, history: &Path) -> PathBuf {
        history.join(format!("{}.json", self.id))
    }
    pub fn save(&mut self, history: &Path) -> io::Result<()> {
        self.updated = now_seconds();
        let mut temp = tempfile::NamedTempFile::new_in(history)?;
        serde_json::to_writer(temp.as_file_mut(), self).map_err(io::Error::other)?;
        temp.as_file().sync_all()?;
        temp.persist(self.receipt(history))
            .map_err(|error| error.error)?;
        Ok(())
    }
    pub fn title(&self, history: &Path) -> String {
        if !self.name.is_empty() {
            return self.name.clone();
        }
        self.wrapper
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .or_else(|| {
                read_offer(&self.offer(history))
                    .ok()
                    .map(|offer| offer.wrapper)
            })
            .unwrap_or_else(|| format!("{} receive", self.mode.label()))
    }
    pub fn location(&self) -> &Path {
        self.wrapper.as_deref().unwrap_or(&self.destination)
    }

    pub fn can_change_code(&self, history: &Path) -> bool {
        read_offer(&self.offer(history)).is_ok_and(|offer| offer.strong)
    }
}

pub fn remember_native(
    history: &Path,
    mode: ReceiveMode,
    ticket: String,
    code: Option<String>,
    destination: PathBuf,
    blob_root: PathBuf,
) -> io::Result<ReceiveEntry> {
    let hash = crate::backend::native_ticket_to_hex_hash(&ticket)
        .ok_or_else(|| io::Error::other("Invalid Sendme ticket"))?;
    fs::create_dir_all(history)?;
    let cache = blob_root.join(format!(".sendme-recv-{hash}"));
    let mut entry = read_entries(history).into_iter().find(|entry| {
        !entry.complete && entry.mode == mode && entry.destination == destination
            && entry.native_cache().as_ref() == Some(&cache)
    }).unwrap_or_else(|| ReceiveEntry {
        id: format!("{:032x}", rand::random::<u128>()),
        mode,
        ticket: None,
        blob_root: Some(blob_root),
        destination,
        original_code: None,
        code: None,
        wrapper: None,
        done: 0,
        total: 0,
        complete: false,
        ready_to_save: false,
        unwrap_on_save: false,
        updated: 0,
        completed_at: None,
        elapsed_ms: None,
        route: None,
        name: String::new(),
        error: String::new(),
    });
    entry.ticket = Some(ticket);
    entry.code = code;
    entry.error.clear();
    entry.save(history)?;
    Ok(entry)
}

pub fn finish_native(history: &Path, entry: &mut ReceiveEntry, output: PathBuf, wrapped: bool, unwrap_on_save: bool) -> io::Result<()> {
    entry.destination = if wrapped {
        output.parent().ok_or_else(|| io::Error::other("Missing receive destination"))?.to_owned()
    } else {
        output.clone()
    };
    if wrapped {
        entry.name = output.file_name().unwrap_or_default().to_string_lossy().into_owned();
    }
    entry.wrapper = wrapped.then_some(output);
    entry.complete = true;
    entry.completed_at = Some(now_seconds());
    entry.ticket = None;
    entry.code = None;
    entry.save(history)?;
    if unwrap_on_save && entry.wrapper.is_some() {
        unwrap(history, entry)?;
    }
    Ok(())
}

fn now_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn read_offer(path: &Path) -> io::Result<Offer> {
    serde_json::from_reader(fs::File::open(path)?).map_err(io::Error::other)
}

pub fn entries(history: &Path) -> Vec<ReceiveEntry> {
    read_entries(history).into_iter().filter(|entry| {
        entry.complete || if entry.mode == ReceiveMode::Croc {
            entry.work().is_dir()
        } else {
            entry.native_cache().is_some_and(|path| path.is_dir())
        }
    }).collect()
}

fn read_entries(history: &Path) -> Vec<ReceiveEntry> {
    let Ok(files) = fs::read_dir(history) else {
        return Vec::new();
    };
    let mut entries: Vec<_> = files
        .flatten()
        .filter_map(|file| {
            let path = file.path();
            if path.extension()?.to_str()? != "json" {
                return None;
            }
            let id = path.file_stem()?.to_str()?;
            if !valid_id(id) {
                return None;
            }
            let entry: ReceiveEntry = serde_json::from_reader(fs::File::open(&path).ok()?).ok()?;
            if entry.id != id {
                return None;
            }
            Some(entry)
        })
        .collect();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.updated));
    entries
}

pub fn discard(history: &Path, entry: &ReceiveEntry) -> io::Result<()> {
    if !valid_id(&entry.id) {
        return Err(io::Error::other("Invalid receive record"));
    }
    if entry.mode != ReceiveMode::Croc {
        if !entry.complete {
            if let (Some(path), Some(root)) = (entry.native_cache(), &entry.blob_root) {
                if fs::symlink_metadata(&path).is_ok_and(|info| info.file_type().is_dir())
                    && path.canonicalize()?.parent() == Some(root.canonicalize()?.as_path())
                {
                    fs::remove_dir_all(path)?;
                }
            }
        }
        return remove_if_present(&entry.receipt(history));
    }
    let work = entry.work();
    if !entry.complete && work.exists() {
        let destination = entry.destination.canonicalize()?;
        if !fs::symlink_metadata(&work)?.file_type().is_dir()
            || work.canonicalize()?.parent() != Some(destination.as_path())
        {
            return Err(io::Error::other("The private receive folder changed"));
        }
        fs::remove_dir_all(work)?;
    }
    remove_if_present(&entry.offer(history))?;
    for suffix in ["review", "review.tmp", "decision", "revision", "revision.tmp", "tmp"] {
        remove_if_present(&history.join(format!("{}.offer.{suffix}", entry.id)))?;
    }
    remove_if_present(&entry.receipt(history))
}

pub fn clear_expired(history: &Path, cutoff: u64) {
    for entry in read_entries(history) {
        let age_from = entry.completed_at.unwrap_or(entry.updated);
        if !entry.ready_to_save && age_from != 0 && age_from <= cutoff {
            let _ = if entry.mode == ReceiveMode::Croc {
                discard(history, &entry)
            } else {
                remove_if_present(&entry.receipt(history))
            };
        }
    }
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

fn next_name(name: &str, number: u32) -> String {
    if number == 0 {
        name.to_owned()
    } else {
        format!("{name} ({number})")
    }
}

fn move_name(name: &OsStr, is_file: bool, number: u32) -> OsString {
    if number == 0 {
        return name.to_owned();
    }
    let path = Path::new(name);
    let mut renamed = if is_file {
        path.file_stem().unwrap_or(name)
    } else {
        name
    }
    .to_owned();
    renamed.push(format!(" ({number})"));
    if let Some(extension) = path.extension().filter(|_| is_file) {
        renamed.push(".");
        renamed.push(extension);
    }
    renamed
}

fn publish(history: &Path, entry: &mut ReceiveEntry) -> io::Result<()> {
    if entry.wrapper.is_none() {
        let offer = read_offer(&entry.offer(history))?;
        if offer.wrapper.is_empty()
            || !offer.wrapper.ends_with("_databeam")
            || offer.wrapper.contains(['/', '\\', ':', '\0'])
        {
            return Err(io::Error::other("Invalid Croc save folder name"));
        }
        if entry.unwrap_on_save {
            entry.name = offer
                .wrapper
                .strip_suffix("_databeam")
                .unwrap_or(&offer.wrapper)
                .to_owned();
        } else {
            let mut chosen = None;
            for number in 0..1000 {
                let path = entry.destination.join(next_name(&offer.wrapper, number));
                match fs::create_dir(&path) {
                    Ok(()) => {
                        chosen = Some(path);
                        break;
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                }
            }
            entry.wrapper =
                Some(chosen.ok_or_else(|| io::Error::other("Too many matching folders"))?);
            entry.save(history)?;
        }
    }
    if let Some(name) = entry.wrapper.as_deref().and_then(Path::file_name) {
        entry.name = name.to_string_lossy().into_owned();
    }
    let target = entry.location();
    let root = fs::symlink_metadata(target)?;
    if !root.file_type().is_dir() {
        return Err(io::Error::other("Save folder changed during transfer"));
    }
    let work = entry.work();
    match fs::symlink_metadata(&work) {
        Ok(info) if info.file_type().is_dir() => move_children(&work, target)?,
        // Older versions removed the empty work folder before saving the completion record.
        Err(error) if error.kind() == io::ErrorKind::NotFound && entry.wrapper.is_some() => {}
        Ok(_) => return Err(io::Error::other("The private receive folder changed")),
        Err(error) => return Err(error),
    }
    entry.complete = true;
    entry.completed_at = Some(now_seconds());
    entry.ready_to_save = false;
    entry.done = entry.total;
    entry.code = None;
    entry.original_code = None;
    entry.error.clear();
    entry.save(history)?;
    let _ = fs::remove_dir(work);
    remove_if_present(&entry.offer(history))
}

fn move_children(source: &Path, target: &Path) -> io::Result<()> {
    for child in fs::read_dir(source)? {
        let child = child?;
        let original = child.file_name();
        let is_file = child.file_type()?.is_file();
        let mut moved = false;
        for number in 0..1000 {
            let destination = target.join(move_name(&original, is_file, number));
            match move_exclusive(&child.path(), &destination) {
                Ok(()) => {
                    moved = true;
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        if !moved {
            return Err(io::Error::other("Too many matching names"));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn move_exclusive(source: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileW(old: *const u16, new: *const u16) -> i32;
    }
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    if unsafe { MoveFileW(wide(source).as_ptr(), wide(target).as_ptr()) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn move_exclusive(source: &Path, target: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let old = CString::new(source.as_os_str().as_bytes())?;
    let new = CString::new(target.as_os_str().as_bytes())?;
    #[cfg(target_os = "linux")]
    let status = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            old.as_ptr(),
            libc::AT_FDCWD,
            new.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(target_os = "macos")]
    let status = unsafe { libc::renamex_np(old.as_ptr(), new.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Safe folder move unavailable",
    ));
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub fn unwrap(history: &Path, entry: &mut ReceiveEntry) -> io::Result<()> {
    let Some(wrapper) = entry.wrapper.clone() else {
        return Ok(());
    };
    if !entry.complete
        || wrapper.parent() != Some(entry.destination.as_path())
        || !fs::symlink_metadata(&wrapper)?.file_type().is_dir()
    {
        return Err(io::Error::other("Saved folder changed"));
    }
    move_children(&wrapper, &entry.destination)?;
    fs::remove_dir(&wrapper)?;
    entry.wrapper = None;
    entry.save(history)
}

pub fn start(
    history: PathBuf,
    destination: PathBuf,
    code: String,
    binary: &str,
    previous: Option<ReceiveEntry>,
    unwrap_on_save: bool,
    relay: Option<String>,
) -> io::Result<(mpsc::Receiver<TransferMsg>, ProcessHandle)> {
    fs::create_dir_all(&history)?;
    let review_changes = previous.as_ref().is_some_and(|entry| entry.can_change_code(&history));
    let mut entry = match previous {
        Some(mut entry) if valid_id(&entry.id) && !entry.complete => {
            if !entry.ready_to_save {
                entry.unwrap_on_save = unwrap_on_save;
            }
            entry
        }
        Some(_) => return Err(io::Error::other("Cannot resume this receive")),
        None => {
            let destination = destination.canonicalize()?;
            let entry = ReceiveEntry {
                id: format!("{:032x}", rand::random::<u128>()),
                mode: ReceiveMode::Croc,
                ticket: None,
                blob_root: None,
                destination,
                original_code: Some(code.clone()),
                code: Some(code.clone()),
                wrapper: None,
                done: 0,
                total: 0,
                complete: false,
                ready_to_save: false,
                unwrap_on_save,
                updated: 0,
                completed_at: None,
                elapsed_ms: None,
                route: None,
                name: String::new(),
                error: String::new(),
            };
            fs::create_dir(entry.work())?;
            entry
        }
    };
    for suffix in ["review", "decision"] {
        remove_if_present(&history.join(format!("{}.offer.{suffix}", entry.id)))?;
    }
    entry.save(&history)?;
    let (tx, rx) = mpsc::channel();
    let (events, handle) = if entry.ready_to_save {
        let (_unused, receiver) = mpsc::channel();
        (
            receiver,
            ProcessHandle {
                cancel: Default::default(),
                child_pid: Default::default(),
                receive_shutdown: None,
            },
        )
    } else {
        let work = entry.work();
        if !fs::symlink_metadata(&work)?.file_type().is_dir() {
            return Err(io::Error::other("The private receive folder is missing"));
        }
        let changed = entry.original_code.as_deref() != Some(code.as_str());
        croc_receive_working(
            CrocReceiveOptions {
                code: code.clone(),
                output_dir: Some(work),
            },
            binary,
            entry.offer(&history),
            changed,
            review_changes,
            relay,
        )
    };
    thread::spawn(move || {
        let result = (|| -> io::Result<()> {
            if !entry.ready_to_save {
                let mut last = Instant::now();
                let mut last_progress = None;
                let mut payload_started = None::<Instant>;
                let elapsed_before = entry.elapsed_ms.unwrap_or(0);
                for event in events {
                    if let Some(started) = payload_started {
                        entry.elapsed_ms = Some(elapsed_before.saturating_add(started.elapsed().as_millis() as u64));
                    }
                    match event {
                        TransferMsg::Completed => {
                            let offer = entry.offer(&history);
                            if !offer.is_file() {
                                // Text uses the regular Croc stdout path.
                                discard(&history, &entry)?;
                                let _ = tx.send(TransferMsg::Completed);
                                return Ok(());
                            }
                            entry.total = read_offer(&offer)?.total;
                            if entry.total == 0 && entry.elapsed_ms.is_none() { entry.elapsed_ms = Some(0); }
                            entry.ready_to_save = true;
                            entry.save(&history)?;
                            break;
                        }
                        TransferMsg::Error(message) => {
                            entry.error.clone_from(&message);
                            let _ = entry.save(&history);
                            let _ = tx.send(TransferMsg::Error(message));
                            return Ok(());
                        }
                        TransferMsg::CrocProgress(progress) => {
                            if last_progress.is_some_and(|previous| progress.done > previous) {
                                payload_started.get_or_insert_with(Instant::now);
                            }
                            last_progress = Some(progress.done);
                            entry.done = progress.done;
                            entry.total = progress.total;
                            entry.code = Some(code.clone());
                            if last.elapsed().as_secs() >= 1 {
                                entry.save(&history)?;
                                last = Instant::now();
                            }
                            let _ = tx.send(event);
                        }
                        TransferMsg::Output(line)
                            if line.trim().starts_with("DATABEAM_OFFER_V1 ") =>
                        {
                            let offer = read_offer(&entry.offer(&history))?;
                            entry.total = offer.total;
                            entry.error.clear();
                            entry.save(&history)?;
                        }
                        TransferMsg::Output(ref line) => {
                            if let Some(route) = parse_croc_direction(line) {
                                entry.route = Some(route);
                            }
                            let _ = tx.send(event);
                        }
                        _ => {
                            let _ = tx.send(event);
                        }
                    }
                }
                if !entry.ready_to_save {
                    entry.save(&history)?;
                }
            }
            if entry.ready_to_save {
                publish(&history, &mut entry)?;
                let _ = tx.send(TransferMsg::Completed);
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = tx.send(TransferMsg::Error(error.to_string()));
        }
    });
    Ok((rx, handle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_unwrap_saves_in_destination_and_preserves_collisions() {
        let temp = tempfile::tempdir().unwrap();
        let history = temp.path().join("history");
        fs::create_dir(&history).unwrap();
        let mut entry = ReceiveEntry {
            id: format!("{:032x}", 1),
            mode: ReceiveMode::Croc,
            ticket: None,
            blob_root: None,
            destination: temp.path().to_owned(),
            original_code: Some("example-code".into()),
            code: Some("example-code".into()),
            wrapper: None,
            done: 0,
            total: 10,
            complete: false,
            ready_to_save: true,
            unwrap_on_save: true,
            updated: 0,
            completed_at: None,
            elapsed_ms: None,
            route: None,
            name: String::new(),
            error: String::new(),
        };
        let work = entry.work();
        fs::create_dir_all(work.join("Photos/empty")).unwrap();
        fs::write(work.join("report.pdf"), b"new report").unwrap();
        fs::write(temp.path().join("report.pdf"), b"previous report").unwrap();
        fs::create_dir(temp.path().join("Photos")).unwrap();
        fs::write(
            entry.offer(&history),
            r#"{"wrapper":"report.pdf_and_more_databeam","total":10,"strong":true}"#,
        )
        .unwrap();

        publish(&history, &mut entry).unwrap();

        assert!(entry.complete && !entry.ready_to_save);
        assert!(entry.wrapper.is_none() && !work.exists() && !entry.offer(&history).exists());
        assert_eq!(entry.location(), temp.path());
        assert_eq!(
            fs::read(temp.path().join("report.pdf")).unwrap(),
            b"previous report"
        );
        assert_eq!(
            fs::read(temp.path().join("report (1).pdf")).unwrap(),
            b"new report"
        );
        assert!(temp.path().join("Photos (1)/empty").is_dir());
        assert!(!temp.path().join("report.pdf_and_more_databeam").exists());
    }

    #[test]
    fn native_history_unwrap_preserves_existing_files_and_drops_the_ticket() {
        let temp = tempfile::tempdir().unwrap();
        let history = temp.path().join("history");
        let destination = temp.path().join("downloads");
        let blobs = temp.path().join("blobs");
        let ticket = "blobacxfr74igmsbvsbnn73wcecg5vt3kbzncqwfrdiampuufwnhkublmaaaaafyju2y4tel43bymjvsdax7k5mbrotl2p2lsbdetff6ctftksqhe";
        let mut entry = remember_native(&history, ReceiveMode::Sendme, ticket.into(), None, destination.clone(), blobs).unwrap();
        fs::create_dir_all(entry.native_cache().unwrap()).unwrap();
        assert!(entries(&history).iter().any(|saved| saved.ticket.as_deref() == Some(ticket)));
        let wrapper = destination.join("report.txt_databeam");
        fs::create_dir_all(wrapper.join("empty")).unwrap();
        fs::write(wrapper.join("report.txt"), "received").unwrap();
        fs::write(destination.join("report.txt"), "existing").unwrap();
        finish_native(&history, &mut entry, wrapper.clone(), true, true).unwrap();
        assert_eq!(fs::read_to_string(destination.join("report.txt")).unwrap(), "existing");
        assert_eq!(fs::read_to_string(destination.join("report (1).txt")).unwrap(), "received");
        assert!(destination.join("empty").is_dir());
        assert!(!wrapper.exists());
        let saved = entries(&history).pop().unwrap();
        assert!(saved.complete && saved.ticket.is_none() && saved.wrapper.is_none());
    }

    #[test]
    #[ignore = "requires DATABEAM_CROC_TEST_BINARY and a reachable Croc relay"]
    fn managed_croc_round_trip_and_move_out() {
        use crate::backend::{croc_send, CrocSendOptions};
        use sha2::{Digest, Sha256};
        use std::time::Duration;

        let binary = std::env::var("DATABEAM_CROC_TEST_BINARY").unwrap();
        let root = tempfile::tempdir().unwrap();
        let history = root.path().join("history");
        let destination = root.path().join("destination");
        let source = root.path().join("Photos");
        fs::create_dir_all(source.join("nested/empty")).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(
            destination.join("Photos_databeam"),
            b"existing wrapper name",
        )
        .unwrap();
        fs::create_dir(destination.join("Photos")).unwrap();
        fs::write(destination.join("Photos/existing.txt"), b"existing folder").unwrap();
        let data: Vec<u8> = (0..524288).map(|_| rand::random()).collect();
        fs::write(source.join("nested/image.bin"), &data).unwrap();
        fs::write(source.join("zero.txt"), b"").unwrap();

        let code =
            std::iter::repeat_with(|| format!("managed-test-{:016x}", rand::random::<u64>()))
                .find(|code| Sha256::digest(code.as_bytes())[31] % 4 == 0)
                .unwrap();
        let (_send_events, sender) = croc_send(
            CrocSendOptions {
                paths: vec![source],
                custom_code: Some(code.clone()),
                text_mode: false,
                text_value: None,
            },
            &binary,
        );
        struct CancelOnDrop(ProcessHandle);
        impl Drop for CancelOnDrop {
            fn drop(&mut self) {
                self.0.request_cancel();
            }
        }
        let _sender = CancelOnDrop(sender);
        let (events, receiver) = start(
            history.clone(),
            destination.clone(),
            code,
            &binary,
            None,
            false,
            None,
        )
        .unwrap();
        let _receiver = CancelOnDrop(receiver);
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            match events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap()
            {
                TransferMsg::Completed => break,
                TransferMsg::Error(error) => panic!("{error}"),
                _ => {}
            }
        }
        let mut entry = entries(&history).pop().unwrap();
        assert!(entry.complete);
        assert!(!entry.work().exists() && !entry.offer(&history).exists());
        assert_eq!(entry.location().file_name().unwrap(), "Photos_databeam (1)");
        assert_eq!(
            fs::read(entry.location().join("Photos/nested/image.bin")).unwrap(),
            data
        );
        assert!(entry.location().join("Photos/nested/empty").is_dir());
        assert_eq!(
            fs::metadata(entry.location().join("Photos/zero.txt"))
                .unwrap()
                .len(),
            0
        );
        unwrap(&history, &mut entry).unwrap();
        assert_eq!(
            fs::read(destination.join("Photos (1)/nested/image.bin")).unwrap(),
            data
        );
        assert_eq!(
            fs::read(destination.join("Photos/existing.txt")).unwrap(),
            b"existing folder"
        );
        assert_eq!(
            fs::read(destination.join("Photos_databeam")).unwrap(),
            b"existing wrapper name"
        );
    }
}
