fn main() {
    // маленький C-модуль (папка native/)
    println!("cargo:rerun-if-changed=ram.c");
    cc::Build::new().file("ram.c").compile("rustx_native");

    println!("cargo:rerun-if-changed=icon.ico");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("icon.ico");
        res.set("ProductName", "rustx");
        res.set("FileDescription", "rustx - Minecraft launcher");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icon was not embedded: {}", e);
        }
    }
}
