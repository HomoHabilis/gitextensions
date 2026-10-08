//! Embeds the Windows resources (icons, version information) into `gitext.exe`.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=res");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = env!("CARGO_PKG_VERSION");
    let mut parts: Vec<&str> = version.split(['.', '-', '+']).take(3).collect();
    parts.resize(3, "0");
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let icons = manifest_dir.join("res").join("icons").display().to_string().replace('\\', "/");
    let rc = std::fs::read_to_string(manifest_dir.join("res").join("gitext.rc"))
        .unwrap()
        .replace("@VERSION_COMMA@", &format!("{},0", parts.join(",")))
        .replace("@VERSION@", version)
        .replace("@ICONS@", &icons);
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("gitext.rc");
    std::fs::write(&out, rc).unwrap();
    embed_resource::compile(&out, embed_resource::NONE).manifest_required().unwrap();
}
