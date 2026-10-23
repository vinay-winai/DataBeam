fn main() {
    prepare_croc_payload();
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");

    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icons/icon.ico");
        if let Err(err) = res.compile() {
            panic!("failed to compile Windows icon resources: {err}");
        }
    }
}

fn prepare_croc_payload() {
    use sha2::{Digest, Sha256};
    use std::{env, fs, path::PathBuf};
    println!("cargo:rerun-if-env-changed=DATABEAM_CROC_BUNDLE_DIR");
    println!("cargo:rerun-if-changed=patches/croc-engine.json");
    println!("cargo:rerun-if-changed=patches/croc-v11.2.5-relay-keepalive.patch");
    let recipe: serde_json::Value = serde_json::from_slice(
        &fs::read("patches/croc-engine.json").expect("read Croc engine recipe"),
    )
    .expect("parse Croc engine recipe");
    let version = recipe["engine_version"]
        .as_str()
        .expect("Croc engine version");
    let target = env::var("TARGET").unwrap();
    let bundle = env::var_os("DATABEAM_CROC_BUNDLE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("target/croc-bundle")
        })
        .join(&target);
    let manifest = bundle.join("manifest.json");
    let gzip = bundle.join("croc.gz");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rerun-if-changed={}", gzip.display());
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let identity = format!(
        "pub const SUPPORTED_CROC_VERSION: &str = {:?};\n",
        format!("v{version}")
    );
    let payload = if manifest.exists() {
        let metadata: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        assert_eq!(
            metadata["version"].as_str(),
            Some(version),
            "stale Croc payload: rerun scripts/prepare_croc.py"
        );
        assert_eq!(
            metadata["target"].as_str(),
            Some(target.as_str()),
            "Croc target mismatch"
        );
        let patch =
            fs::read(PathBuf::from("patches").join(recipe["patch"].as_str().unwrap())).unwrap();
        let patch_hash: String = Sha256::digest(patch)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(
            metadata["patch_sha256"].as_str(),
            Some(patch_hash.as_str()),
            "stale Croc patch: rerun scripts/prepare_croc.py"
        );
        let bytes = fs::read(&gzip).expect("read prepared Croc payload");
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(
            metadata["gzip_sha256"].as_str(),
            Some(hash.as_str()),
            "Croc payload SHA-256 mismatch"
        );
        let binary_hash = metadata["binary_sha256"]
            .as_str()
            .expect("Croc binary SHA-256");
        assert!(binary_hash.len() == 64 && binary_hash.bytes().all(|b| b.is_ascii_hexdigit()));
        let embedded = out.join("croc.gz");
        fs::write(&embedded, bytes).unwrap();
        format!("const CROC_GZ: &[u8] = include_bytes!({:?});\nconst CROC_BINARY_SHA256: &str = {:?};\n", embedded, binary_hash)
    } else {
        assert_ne!(env::var("PROFILE").unwrap(), "release",
            "Prepare the patched Croc engine first: python scripts/prepare_croc.py --target {target}");
        println!("cargo:warning=No Croc bundle prepared; this development build needs a matching patched system engine. Run python scripts/prepare_croc.py --target {target}");
        "const CROC_GZ: &[u8] = &[];\nconst CROC_BINARY_SHA256: &str = \"\";\n".to_string()
    };
    fs::write(out.join("croc_payload.rs"), identity + &payload).unwrap();
}
