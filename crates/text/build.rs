//! Embeds the optional craft-fonts build input (`CRAFT_FONTS_DIR`): both `fonts/manifest.txt` and the
//! opt-in Latin presentation fonts in `fonts/latin-manifest.txt` (Inter, Roboto, …). Unset or missing,
//! `CRAFT_FONTS` is empty and the app uses system fonts. Recipe: craft-fonts `docs/integration.md`.
//! Noto Sans CJK is included in web builds; desktop builds opt in with `FIGCRAFT_EMBED_CJK`.
use std::fmt::Write as _;
use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_REQUIRED");
    println!("cargo::rerun-if-env-changed=FIGCRAFT_EMBED_CJK");
    let mut src = String::from("pub static CRAFT_FONTS: &[CraftFont] = &[\n");
    if let Some(dir) = std::env::var_os("CRAFT_FONTS_DIR").map(PathBuf::from) {
        let mut ok = false;
        for manifest in ["fonts/latin-manifest.txt", "fonts/manifest.txt"] {
            match craft_fonts(&dir, manifest) {
                Ok(entries) => {
                    ok = true;
                    src.push_str(&entries);
                }
                Err(e) if std::env::var_os("CRAFT_FONTS_REQUIRED").is_some() => {
                    println!("cargo::error=CRAFT_FONTS_DIR={}: {e}", dir.display());
                }
                Err(e) => println!("cargo::warning=craft-fonts {manifest} skipped: CRAFT_FONTS_DIR={}: {e}", dir.display()),
            }
        }
        if !ok {
            println!("cargo::warning=building without craft-fonts");
        }
    }
    src.push_str("];\n");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("craft_fonts.rs");
    if let Err(e) = std::fs::write(&out, src) {
        println!("cargo::error=writing {}: {e}", out.display());
    }
}

fn craft_fonts(dir: &std::path::Path, manifest_rel: &str) -> Result<String, String> {
    let manifest = dir.join(manifest_rel);
    println!("cargo::rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let wasm = std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|a| a == "wasm32");
    let mut out = String::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let f: Vec<&str> = line.split(" | ").map(str::trim).collect();
        let [family, style, file, scripts, ..] = f.as_slice() else {
            return Err(format!("malformed manifest line: {line}"));
        };
        // CJK fonts are large; desktop apps find system CJK fonts instead unless asked. Web apps
        // have no system fonts, so include Noto Sans CJK by default.
        let cjk = scripts.contains("Hans");
        if cjk && !wasm && std::env::var_os("FIGCRAFT_EMBED_CJK").is_none() {
            continue;
        }
        // Web builds have a size budget: Inter plus Noto Sans CJK for Chinese, Japanese and Korean.
        if wasm && *family != "Inter" && !cjk {
            continue;
        }
        let path = dir.join(file).canonicalize().map_err(|e| format!("{file}: {e}"))?;
        println!("cargo::rerun-if-changed={}", path.display());
        let _ = writeln!(out, "    CraftFont {{ family: {family:?}, style: {style:?}, bytes: include_bytes!({:?}) }},", path.display().to_string());
    }
    Ok(out)
}
