//! Windows only: embed the app icon and version info (VERSIONINFO) into `cella.exe`, so it
//! shows in Explorer, the taskbar, the Start menu and Alt-Tab.
//!
//! On every other target this does nothing. A missing resource compiler is a warning, so a
//! cross-compile from macOS or Linux still links, unless `GRIDCRAFT_REQUIRE_WINRES=1` turns it
//! into an error (for release builds).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/app-icon/cella.ico");
    println!("cargo:rerun-if-env-changed=CELLA_REQUIRE_WINRES");
    println!("cargo:rerun-if-env-changed=GRIDCRAFT_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/app-icon/cella.ico")
        .set("ProductName", "Cella")
        .set("FileDescription", "Cella spreadsheet")
        .set("CompanyName", "Learning Machines LLC")
        .set("LegalCopyright", "Copyright (c) the Cella authors. MIT OR Apache-2.0.")
        .set("OriginalFilename", "cella.exe")
        .set("InternalName", "cella");
    if let Err(e) = res.compile() {
        // The repository renamed from GridCraft to Cella; the packager already sets the new
        // name, and the old one is still honoured so an old CI recipe keeps working.
        if std::env::var_os("CELLA_REQUIRE_WINRES").is_some() || std::env::var_os("GRIDCRAFT_REQUIRE_WINRES").is_some() {
            eprintln!("embedding Windows resources failed: {e}");
            std::process::exit(1);
        }
        println!("cargo:warning=cella.exe built without icon/version resources: {e}");
    }
}
