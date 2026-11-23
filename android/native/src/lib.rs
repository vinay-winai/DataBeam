use jni::{
    objects::{JClass, JString},
    sys::{jboolean, jstring},
    JNIEnv,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
static CANCEL: OnceLock<Mutex<Option<Arc<AtomicBool>>>> = OnceLock::new();
static EVENTS: OnceLock<Mutex<Vec<Value>>> = OnceLock::new();
#[cfg(target_os = "android")]
static ANDROID_DNS_CONTEXT: OnceLock<jni22::refs::Global<jni22::objects::JObject<'static>>> = OnceLock::new();
#[cfg(target_os = "android")]
static ANDROID_DNS_INIT: std::sync::Once = std::sync::Once::new();
fn event(kind: &str, payload: &str) {
    let mut q = EVENTS.get_or_init(Default::default).lock().unwrap();
    queue_event(&mut q, kind, payload);
}
fn queue_event(q: &mut Vec<Value>, kind: &str, payload: &str) {
    if matches!(kind, "receive-progress" | "transfer-progress" | "preparation-progress") {
        let samples: Vec<_> = q.iter().enumerate().filter(|(_, e)| e["kind"] == kind).map(|(i, _)| i).collect();
        if let Some(&last) = samples.last() {
            let previous = q[last]["payload"].as_str().unwrap_or("");
            if previous.split(':').next() == payload.split(':').next() {
                q[last]["payload"] = json!(payload);
                return;
            }
            // Keep the baseline, first advancing sample (first-byte timing),
            // and latest sample. Coalesce later samples without reordering any
            // route changes or completion events relative to the latest one.
            if samples.len() >= 3 { q.remove(last); }
        }
    }
    if q.len() >= 256 {
        // Progress/log bursts must not evict a route change or completion before
        // Android's next poll. These samples can be superseded safely.
        let discard = q.iter().position(|e| {
            matches!(e["kind"].as_str(), Some("log" | "receive-progress" | "transfer-progress" | "preparation-progress" | "send-paths" | "receive-paths"))
        }).unwrap_or(0);
        q.remove(discard);
    }
    let at_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    q.push(json!({"kind":kind,"payload":payload,"atMs":at_ms}));
}
#[cfg(test)]
mod event_queue_tests {
    #[test]
    fn progress_burst_retains_path_switch_and_completion() {
        let mut q = Vec::new();
        super::queue_event(&mut q, "receive-route", "relay");
        super::queue_event(&mut q, "receive-route", "direct");
        super::queue_event(&mut q, "transfer-completed", "");
        for n in 0..1000 { super::queue_event(&mut q, "receive-progress", &format!("{n}:1000:0")); }
        assert!(q.len() <= 256);
        let samples: Vec<_> = q.iter().filter(|e| e["kind"] == "receive-progress").map(|e| e["payload"].as_str().unwrap()).collect();
        assert_eq!(samples, ["0:1000:0", "1:1000:0", "999:1000:0"]);
        let routes: Vec<_> = q.iter().filter(|e| e["kind"] == "receive-route").map(|e| e["payload"].as_str().unwrap()).collect();
        assert_eq!(routes, ["relay", "direct"]);
        assert!(q.iter().any(|e| e["kind"] == "transfer-completed"));
    }
}
struct Emitter;
#[no_mangle]
pub extern "system" fn Java_com_vinaywinai_databeam_NativeBridge_inspectCache(
    mut env: JNIEnv, _: JClass, ticket: JString, base: JString,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> anyhow::Result<Value> {
        let ticket: String = env.get_string(&ticket)?.into();
        let base: String = env.get_string(&base)?.into();
        let cache = tokio::runtime::Runtime::new()?.block_on(sendme::core::cache::inspect(&ticket, std::path::Path::new(&base)))?;
        Ok(serde_json::to_value(cache)?)
    }));
    let value = match result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => json!({"error":format!("{error:#}")}),
        Err(_) => json!({"error":"Could not inspect saved receive data"}),
    };
    env.new_string(value.to_string()).map(|s|s.into_raw()).unwrap_or(std::ptr::null_mut())
}
fn check_sendme_relay() -> anyhow::Result<Value> {
    tokio::runtime::Runtime::new()?.block_on(async {
        let endpoint=iroh::Endpoint::builder(iroh::endpoint::presets::N0).bind().await?;
        let online=tokio::time::timeout(Duration::from_secs(12),endpoint.online()).await;
        let relays=endpoint.addr().relay_urls().map(ToString::to_string).collect::<Vec<_>>();
        endpoint.close().await;
        match online {
            Ok(()) if !relays.is_empty()=>Ok(json!({"reachable":true,"relays":relays})),
            _=>Ok(json!({"reachable":false,"message":"No Sendme public relay connection established within 12 seconds from this phone."})),
        }
    })
}

#[no_mangle]
pub extern "system" fn Java_com_vinaywinai_databeam_NativeBridge_checkSendmeRelay(
    env: JNIEnv,
    _: JClass,
) -> jstring {
    let value = std::panic::catch_unwind(check_sendme_relay);
    let result = match value {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => json!({"reachable":false,"message":format!("{error:#}")}),
        Err(_) => json!({"reachable":false,"message":"Sendme relay check failed"}),
    };
    env.new_string(result.to_string())
        .map(|value| value.into_raw())
        .unwrap_or(std::ptr::null_mut())
}
impl sendme::EventEmitter for Emitter {
    fn emit_event(&self, n: &str) -> Result<(), String> {
        event(n, "");
        Ok(())
    }
    fn emit_event_with_payload(&self, n: &str, p: &str) -> Result<(), String> {
        event(n, p);
        Ok(())
    }
}
#[derive(Deserialize)]
struct Config {
    action: String,
    #[serde(default)]
    paths: Vec<PathBuf>,
    #[serde(default)]
    ticket: String,
    cache: PathBuf,
    output: PathBuf,
    #[serde(default = "root_name")]
    root: String,
    #[cfg(all(target_os = "android", feature = "descriptor-sources"))]
    #[serde(default, rename = "sourceRoot")]
    source_root: PathBuf,
    #[cfg(all(target_os = "android", feature = "descriptor-sources"))]
    #[serde(default, rename = "sourceDescriptors")]
    source_descriptors: std::collections::HashMap<PathBuf, i32>,
}
fn root_name() -> String {
    "DataBeam_databeam".into()
}
#[cfg(all(target_os = "android", feature = "descriptor-sources"))]
fn source_files(config: &Config) -> anyhow::Result<Arc<std::collections::HashMap<PathBuf, Arc<std::fs::File>>>> {
    use std::{collections::HashMap, fs::File, os::fd::FromRawFd};

    let mut files = HashMap::new();
    if config.source_descriptors.is_empty() {
        return Ok(Arc::new(files));
    }
    let root = config.source_root.canonicalize()?;
    for (path, &fd) in &config.source_descriptors {
        anyhow::ensure!(path.is_absolute() && fd >= 0, "Invalid selected file descriptor");
        let parent = path.parent().ok_or_else(|| anyhow::anyhow!("Selected file has no parent"))?;
        anyhow::ensure!(parent.starts_with(&root) && parent.canonicalize()? == parent,
            "Selected file is outside the transfer source folder");
        anyhow::ensure!(std::fs::read_link(path)? == PathBuf::from(format!("databeam-fd:{fd}")),
            "Selected file descriptor changed");
        // Kotlin holds the original until this operation ends. Own a duplicate
        // so the store can finish reads and close its handles independently.
        let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
        if duplicate < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let file = unsafe { File::from_raw_fd(duplicate) };
        anyhow::ensure!(file.metadata()?.is_file(), "Selected source is not a regular file");
        files.insert(path.clone(), Arc::new(file));
    }
    Ok(Arc::new(files))
}

#[no_mangle]
pub extern "system" fn Java_com_vinaywinai_databeam_NativeBridge_supportsSendmeFileDescriptors(
    _: JNIEnv, _: JClass,
) -> jboolean {
    u8::from(cfg!(all(target_os = "android", feature = "descriptor-sources")))
}

fn run(c: Config) -> anyhow::Result<Value> {
    let cancel = Arc::new(AtomicBool::new(STOP_REQUESTED.load(Ordering::SeqCst)));
    {
        let mut current = CANCEL.get_or_init(Default::default).lock().unwrap();
        anyhow::ensure!(
            current.is_none(),
            "Another native operation is still running"
        );
        *current = Some(cancel.clone());
    }
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            *CANCEL.get().unwrap().lock().unwrap() = None;
        }
    }
    let _guard = Clear;
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let app: sendme::AppHandle = Some(Arc::new(Emitter));
        if c.action == "send" {
            #[cfg(all(target_os = "android", feature = "descriptor-sources"))]
            let source_files = source_files(&c)?;
            let share = sendme::start_share(
                c.paths,
                c.root,
                sendme::SendOptions {
                    blob_dir: Some(c.cache),
                    cancel: Some(cancel.clone()),
                    ticket_type: sendme::AddrInfoOptions::RelayAndAddresses,
                    #[cfg(all(target_os = "android", feature = "descriptor-sources"))]
                    source_files,
                    ..Default::default()
                },
                app,
            )
            .await?;
            event("ticket", &share.ticket);
            // A failed receiver does not destroy the share. Keep serving until the user stops.
            while !cancel.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            share.router.shutdown().await?;
            share._store.shutdown().await?;
            Ok(json!({"status":"stopped"}))
        } else {
            anyhow::ensure!(c.action == "receive", "Unknown action");
            let result = sendme::download(
                c.ticket,
                sendme::ReceiveOptions {
                    output_dir: Some(c.output),
                    blob_dir: Some(c.cache),
                    overwrite: false,
                    cancel: Some(cancel),
                    ..Default::default()
                },
                app,
            )
            .await?;
            Ok(json!({"status":"complete","path":result.file_path}))
        }
    })
}
#[no_mangle]
pub extern "system" fn Java_com_vinaywinai_databeam_NativeBridge_run(
    mut env: JNIEnv,
    _: JClass,
    input: JString,
) -> jstring {
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> anyhow::Result<Value> {
            let text: String = env.get_string(&input)?.into();
            run(serde_json::from_str(&text)?)
        }));
    let value = match result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => json!({"status":"error","message":format!("{e:#}")}),
        Err(_) => json!({"status":"error","message":"Native engine panic"}),
    };
    env.new_string(value.to_string())
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}
#[no_mangle]
pub extern "system" fn Java_com_vinaywinai_databeam_NativeBridge_poll(
    env: JNIEnv,
    _: JClass,
) -> jstring {
    let values = std::mem::take(&mut *EVENTS.get_or_init(Default::default).lock().unwrap());
    env.new_string(serde_json::to_string(&values).unwrap())
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}
#[no_mangle]
pub extern "system" fn Java_com_vinaywinai_databeam_NativeBridge_cancel(_: JNIEnv, _: JClass) {
    STOP_REQUESTED.store(true, Ordering::SeqCst);
    if let Some(c) = CANCEL
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .as_ref()
    {
        c.store(true, Ordering::Relaxed);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_vinaywinai_databeam_NativeBridge_hash(
    mut env: JNIEnv,
    _: JClass,
    ticket: JString,
) -> jstring {
    let text = env
        .get_string(&ticket)
        .map(|s| String::from(s))
        .unwrap_or_default();
    let hash = sendme::ticket_to_hex_hash(&text).unwrap_or_default();
    env.new_string(hash)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[cfg(target_os = "android")]
#[jni22::jni_mangle("com.vinaywinai.databeam.NativeBridge")]
pub fn initialize<'caller>(
    mut env: jni22::EnvUnowned<'caller>,
    _: jni22::objects::JClass<'caller>,
    context: jni22::objects::JObject<'caller>,
) {
    use jni22::errors::ThrowRuntimeExAndDefault;
    env.with_env(|env| {
        let reference = env.new_global_ref(&context)?;
        let vm = env.get_java_vm()?;
        let held = ANDROID_DNS_CONTEXT.get_or_init(|| reference);
        // ndk-context borrows these pointers for the entire process lifetime.
        // Keep a global application reference and initialize it exactly once.
        ANDROID_DNS_INIT.call_once(|| unsafe {
            iroh::dns::install_android_jni_context(vm.get_raw().cast(), held.as_obj().as_raw().cast());
        });
        rustls_platform_verifier::android::init_with_env(env, context)
    })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[no_mangle]
pub extern "system" fn Java_com_vinaywinai_databeam_NativeBridge_networkChanged(_: JNIEnv, _: JClass) {
    sendme::notify_network_change();
}

#[no_mangle]
pub extern "system" fn Java_com_vinaywinai_databeam_NativeBridge_prepare(_: JNIEnv, _: JClass) {
    STOP_REQUESTED.store(false, Ordering::SeqCst);
    EVENTS.get_or_init(Default::default).lock().unwrap().clear();
}
