//! 编译期生成 Tauri 上下文（配置、图标、权限）。

fn main() {
    // tauri-build 默认只给可执行文件嵌入 Windows 应用清单，测试二进制缺少
    // Common-Controls v6 依赖会在启动时报 STATUS_ENTRYPOINT_NOT_FOUND。
    // 这里关掉它自带的清单，改为对所有链接产物（含测试）统一嵌入。
    let attrs = tauri_build::Attributes::new()
        .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    tauri_build::try_build(attrs).expect("tauri-build 失败");

    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows-app-manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
