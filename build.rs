fn main() {
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
