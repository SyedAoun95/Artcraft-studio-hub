// Windows: embed the Studio Hub icon and version info in studio-hub.exe (needs rc.exe from the
// Windows SDK, which only a Windows host has). Other builds need nothing here.
fn main() {
    println!("cargo:rerun-if-changed=assets/icons/StudioHub.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/StudioHub.ico").set("ProductName", "Studio Hub").set("FileDescription", "Studio Hub");
        res.compile().expect("embedding the Windows icon failed (is the Windows SDK installed?)");
    }
}
