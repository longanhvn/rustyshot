fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/icon.png");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "RustyShot");
        res.set("FileDescription", "RustyShot - Screenshot, Recorder & Annotation");
        if let Err(e) = res.compile() {
            println!("cargo:warning=winresource compile failed: {e}");
        }
    }
}
