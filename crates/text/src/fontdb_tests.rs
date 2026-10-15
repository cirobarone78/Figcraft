use super::*;

fn pending(styles: &[&str]) -> FontDb {
    let mut db = FontDb::default();
    let entries = styles.iter().map(|s| Pending { path: format!("no-such-dir/{s}.ttf").into(), index: 0, style: (*s).into() }).collect();
    db.pending.insert("cjk ui".into(), ("CJK UI".into(), entries));
    db
}

fn left(db: &FontDb) -> Vec<String> {
    db.pending.get("cjk ui").map(|(_, v)| v.iter().map(|e| e.style.clone()).collect()).unwrap_or_default()
}

#[test]
fn find_one_reads_only_the_nearest_style() {
    // The files don't exist: what matters is which entry is tried, and that the rest stay pending.
    let mut db = pending(&["Bold", "Light", "Regular", "Italic"]);
    assert!(db.find_one("CJK UI", "Regular").is_none());
    assert_eq!(left(&db), ["Bold", "Light", "Italic"]);
    assert!(db.has_family("cjk ui"), "the family is still offered");
    // No exact name: the nearest weight, upright before italic.
    let mut db = pending(&["W6", "Italic", "W3", "Bold"]);
    db.find_one("CJK UI", "Regular");
    assert_eq!(left(&db), ["W6", "Italic", "Bold"]);
    // `find` reads the whole family.
    db.find("CJK UI", "Regular");
    assert!(left(&db).is_empty() && !db.has_family("cjk ui"));
    assert_eq!(pending_weight("W3"), 300);
    assert_eq!(pending_weight("SemiBold"), 600);
    assert_eq!(pending_weight("W12"), 400);
}

#[test]
fn system_fonts_are_scanned_once() {
    // Racing callers (the desktop's background scan, on-demand lookups, tests) share one walk.
    let threads: Vec<_> = (0..4).map(|_| std::thread::spawn(scan_system_fonts)).collect();
    for t in threads {
        t.join().unwrap();
    }
    scan_system_fonts_on_demand();
    let _ = db();
    scan_system_fonts();
    let walked = !system_fonts_opted_out();
    assert_eq!(SCANS.load(Ordering::Relaxed), usize::from(walked));
    assert_eq!(system_fonts_scanned(), walked);
}

#[test]
fn the_scan_reads_names_like_a_whole_file_read() {
    // Every installed font file (none on CI): the partial read finds the same faces and names.
    let mut files = Vec::new();
    for dir in system_font_dirs() {
        let mut stack = vec![dir];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| ["ttf", "otf", "ttc", "otc"].contains(&x.to_ascii_lowercase().to_str().unwrap_or(""))) {
                    files.push(p);
                }
            }
        }
    }
    for p in files {
        let Ok(whole) = std::fs::read(&p) else { continue };
        assert_eq!(faces_in(&index_bytes(&p, whole.len()).unwrap()), faces_in(&whole), "{}", p.display());
    }
}

#[cfg(target_os = "linux")]
struct FontDirectory(std::path::PathBuf);
#[cfg(target_os = "linux")]
impl FontDirectory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let next = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("figcraft-configured-fonts-{}-{next}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
#[cfg(target_os = "linux")]
impl Drop for FontDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn configured_files_preserve_priority_aliases_and_the_existing_depth_policy() {
    let Some(font) = CRAFT_FONTS.iter().find(|font| !faces_in(font.bytes).is_empty()) else {
        eprintln!("skipping real-font indexing fixture: optional CRAFT_FONTS input is empty");
        return;
    };
    let directory = FontDirectory::new();
    let conventional = directory.0.join("a");
    std::fs::create_dir(&conventional).unwrap();
    let first = conventional.join("font.ttf");
    std::fs::write(&first, font.bytes).unwrap();
    let mut deep = directory.0.join("b");
    for _ in 0..9 {
        deep = deep.join("nested");
    }
    std::fs::create_dir_all(&deep).unwrap();
    let second = deep.join("font.ttf");
    std::fs::write(&second, font.bytes).unwrap();
    let broken = directory.0.join("broken.otf");
    std::os::unix::fs::symlink("missing.ttf", &broken).unwrap();
    let alias = directory.0.join("linked.otf");
    std::os::unix::fs::symlink(&second, &alias).unwrap();
    let mut found = BTreeMap::new();
    scan_dir(&conventional, 0, &mut found);
    let mut bounded = BTreeMap::new();
    scan_dir(&directory.0.join("b"), 0, &mut bounded);
    assert!(bounded.is_empty(), "conventional directory depth policy is unchanged");
    index_configured_files(vec![first.clone(), second.clone(), broken, alias.clone()], &mut found);
    let mut expected = BTreeMap::<String, usize>::new();
    for (_, family, _) in faces_in(font.bytes) {
        *expected.entry(family.to_lowercase()).or_default() += 1;
    }
    for (family, faces) in expected {
        let paths: Vec<_> = found[&family].1.iter().map(|face| face.path.clone()).collect();
        let expected: Vec<_> = [&first, &second, &alias].into_iter().flat_map(|path| std::iter::repeat_n(path.clone(), faces)).collect();
        assert_eq!(paths, expected, "existing fonts stay first; exact duplicates vanish but aliases remain");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn the_opt_out_prevents_both_system_scan_entry_points() {
    const VERIFIED: &str = "FIGCRAFT_OPTOUT_VERIFIED";
    if std::env::var_os("FIGCRAFT_TEST_OPTOUT_CHILD").is_some() {
        let mut local = FontDb::default();
        local.scan_system();
        assert!(!local.system_scanned && local.pending.is_empty());
        scan_system_fonts();
        assert_eq!(SCANS.load(Ordering::Relaxed), 0);
        assert!(!system_fonts_scanned());
        println!("{VERIFIED}");
        return;
    }
    let module = module_path!().split_once("::").unwrap().1;
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", &format!("{module}::the_opt_out_prevents_both_system_scan_entry_points"), "--nocapture"]);
    command.env("FIGCRAFT_TEST_OPTOUT_CHILD", "1").env("FIGCRAFT_NO_SYSTEM_FONTS", "1");
    // The production query supervisor also bounds and reaps this isolated test child.
    let output = fontconfig::test_command(command).unwrap();
    assert!(String::from_utf8(output).unwrap().lines().any(|line| line == VERIFIED), "the exact test must actually run");
}
