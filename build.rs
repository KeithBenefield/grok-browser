fn main() {
    // Rebuild if the icon changes.
    println!("cargo:rerun-if-changed=assets/icon.ico");

    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Grok Browser");
        res.set("FileDescription", "Grok Browser");
        // Shown in Explorer / Task Manager for the .exe
        if let Err(err) = res.compile() {
            // Don't hard-fail on exotic hosts; window icon can still load from file.
            eprintln!("cargo:warning=winres failed to embed icon: {err}");
        }
    }
}
