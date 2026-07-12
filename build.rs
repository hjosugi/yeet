fn main() {
    println!("cargo:rerun-if-changed=assets/windows/yeet.manifest");
    println!("cargo:rerun-if-changed=assets/windows/yeet.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest = include_str!("assets/windows/yeet.manifest").replace(
            "@VERSION@",
            &format!("{}.0", std::env::var("CARGO_PKG_VERSION").unwrap()),
        );
        let mut resource = winresource::WindowsResource::new();
        resource
            .set("ProductName", "Yeet")
            .set("InternalName", "yeet.exe")
            .set("OriginalFilename", "yeet.exe")
            .set_icon("assets/windows/yeet.ico")
            .set_manifest(&manifest);
        resource.compile().expect("compile Windows resources");
    }
}
