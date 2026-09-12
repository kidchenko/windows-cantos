fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set_manifest_file("assets/app.manifest");
        res.set("FileDescription", "Cantos");
        res.set("ProductName", "Cantos");
        res.set("LegalCopyright", "MIT licensed");
        if let Err(e) = res.compile() {
            eprintln!("cargo:warning=resource compile failed: {e}");
        }
    }
    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=src/ui/index.html");
}
