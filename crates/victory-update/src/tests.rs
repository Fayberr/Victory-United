use crate::http::Http;
use crate::*;
use sha2::{Digest, Sha256};
use std::io::Write;

fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (n, b) in files {
        w.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(b).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn engine_for(bytes: &[u8], build: &str) -> Engine {
    let sha: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
    Engine { build: build.into(), translator: "1".into(), kind: "release".into(), url: "mem://zip".into(), sha256: sha, size: bytes.len() as u64, notes: "n".into() }
}

struct Fake(std::collections::HashMap<String, Vec<u8>>);
impl Http for Fake {
    fn get(&self, url: &str, sink: &mut dyn FnMut(&[u8], Option<u64>) -> bool) -> Result<()> {
        let body = self.0.get(url).ok_or_else(|| Error::Offline("no route".into()))?;
        for c in body.chunks(7) {
            if !sink(c, Some(body.len() as u64)) {
                return Err(Error::Cancelled);
            }
        }
        Ok(())
    }
}

fn manifest_json(e: &Engine) -> String {
    serde_json::to_string(&Manifest { schema: 1, launcher_version: "0.1.0".into(), launcher_url: String::new(), channels: [("stable".to_string(), Channel { engines: vec![e.clone()] })].into() }).unwrap()
}

#[test]
fn manifest_parses_and_rejects() {
    let e = engine_for(b"x", "0123456789abcdef");
    let m = Manifest::parse(&manifest_json(&e)).unwrap();
    assert_eq!(m.engine("stable", "release"), Some(&e));
    assert_eq!(folder_name(&e), "01234567-release");
    assert!(Manifest::parse(&manifest_json(&e).replace("\"schema\":1", "\"schema\":9")).is_err());
    assert!(Manifest::parse(&manifest_json(&e).replace(&e.sha256, "zz")).is_err());
    assert!(Manifest::parse("not json").is_err());
}

#[test]
fn finds_the_manifest_asset() {
    let one = r#"{"prerelease":false,"assets":[{"name":"a.zip","browser_download_url":"u1"},{"name":"manifest.json","browser_download_url":"u2"}]}"#;
    assert_eq!(parse_release_assets(one, false).unwrap(), "u2");
    let list = r#"[{"prerelease":false,"assets":[]},{"prerelease":true,"assets":[{"name":"manifest.json","browser_download_url":"dev"}]}]"#;
    assert_eq!(parse_release_assets(list, true).unwrap(), "dev");
    assert!(parse_release_assets(list, false).is_err()); // newest stable has no manifest
    assert!(parse_release_assets("[]", true).is_err());
}

#[test]
fn check_with_a_fake_server() {
    let zip = zip_bytes(&[("victory.exe", b"MZ")]);
    let e = engine_for(&zip, "0123456789abcdef");
    let mut routes = std::collections::HashMap::new();
    routes.insert(
        format!("https://api.github.com/repos/{REPO}/releases/latest"),
        br#"{"assets":[{"name":"manifest.json","browser_download_url":"mem://m"}]}"#.to_vec(),
    );
    routes.insert("mem://m".into(), manifest_json(&e).into_bytes());
    let http = Fake(routes);
    let u = check(&http, "stable", "release", &[]).unwrap();
    assert!(u.newer);
    assert_eq!(u.folder, "01234567-release");
    assert!(!check(&http, "stable", "release", &["01234567-release".into()]).unwrap().newer);
    assert!(matches!(check(&Fake(Default::default()), "stable", "release", &[]), Err(Error::Offline(_))));
}

#[test]
fn download_verifies_and_install_flips_last() {
    let dir = tempfile::tempdir().unwrap();
    let zip = zip_bytes(&[("victory.exe", b"MZ"), ("D3D12/x.dll", b"d")]);
    let e = engine_for(&zip, "aaaaaaaa1");
    let http = Fake([("mem://zip".to_string(), zip.clone())].into());
    let dest = dir.path().join("dl").join("e.zip");
    let mut last = (0, 0);
    download(&http, &e, &dest, &mut |d, t| { last = (d, t); true }).unwrap();
    assert_eq!(last, (zip.len() as u64, zip.len() as u64));
    let root = dir.path().join("engine");
    install(&dest, &root, "aaaaaaaa-release").unwrap();
    assert_eq!(std::fs::read_to_string(root.join("current.txt")).unwrap(), "aaaaaaaa-release");
    assert!(root.join("aaaaaaaa-release/D3D12/x.dll").is_file());
    assert!(!root.join("aaaaaaaa-release.partial").exists());
}

#[test]
fn corrupted_or_cancelled_download_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let zip = zip_bytes(&[("victory.exe", b"MZ")]);
    let e = engine_for(&zip, "bbbbbbbb");
    let mut bad = zip.clone();
    bad[10] ^= 0xff;
    let dest = dir.path().join("e.zip");
    let r = download(&Fake([("mem://zip".to_string(), bad)].into()), &e, &dest, &mut |_, _| true);
    assert!(matches!(r, Err(Error::Corrupt(_))));
    assert!(!dest.exists() && !dest.with_extension("part").exists());
    let short = zip[..zip.len() - 1].to_vec();
    assert!(matches!(download(&Fake([("mem://zip".to_string(), short)].into()), &e, &dest, &mut |_, _| true), Err(Error::Corrupt(_))));
    let r = download(&Fake([("mem://zip".to_string(), zip)].into()), &e, &dest, &mut |_, _| false);
    assert!(matches!(r, Err(Error::Cancelled)));
    assert!(!dest.exists() && !dest.with_extension("part").exists());
}

#[test]
fn interrupted_install_leaves_current_alone() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("engine");
    let good = dir.path().join("good.zip");
    std::fs::write(&good, zip_bytes(&[("victory.exe", b"MZ")])).unwrap();
    install(&good, &root, "one-release").unwrap();
    // No victory.exe in the package.
    let bad = dir.path().join("bad.zip");
    std::fs::write(&bad, zip_bytes(&[("readme.txt", b"x")])).unwrap();
    assert!(install(&bad, &root, "two-release").is_err());
    // Not a ZIP at all (cut off download).
    let cut = dir.path().join("cut.zip");
    std::fs::write(&cut, b"PK\x03\x04garbage").unwrap();
    assert!(install(&cut, &root, "three-release").is_err());
    // Path traversal.
    let evil = dir.path().join("evil.zip");
    std::fs::write(&evil, zip_bytes(&[("victory.exe", b"MZ"), ("../escape.txt", b"x")])).unwrap();
    assert!(install(&evil, &root, "four-release").is_err());
    assert!(!dir.path().join("escape.txt").exists());
    assert_eq!(std::fs::read_to_string(root.join("current.txt")).unwrap(), "one-release");
    let names: Vec<_> = installed(&root).into_iter().map(|b| b.name).collect();
    assert_eq!(names, vec!["one-release"]);
    assert!(!root.join("two-release.partial").exists());
}

#[test]
fn single_top_folder_is_stripped() {
    let dir = tempfile::tempdir().unwrap();
    let z = dir.path().join("e.zip");
    std::fs::write(&z, zip_bytes(&[("pkg/victory.exe", b"MZ"), ("pkg/a/b.dll", b"d")])).unwrap();
    let root = dir.path().join("engine");
    install(&z, &root, "x-release").unwrap();
    assert!(root.join("x-release/a/b.dll").is_file());
}

#[test]
fn rollback_and_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("engine");
    let z = dir.path().join("e.zip");
    std::fs::write(&z, zip_bytes(&[("victory.exe", b"MZ")])).unwrap();
    for n in ["a-release", "b-release", "c-release", "d-release", "e-release"] {
        install(&z, &root, n).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    assert_eq!(std::fs::read_to_string(root.join("previous.txt")).unwrap(), "d-release");
    std::fs::create_dir_all(root.join("junk.partial")).unwrap();
    let removed = cleanup(&root, 3);
    assert!(removed.contains(&"junk.partial".to_string()));
    let mut left: Vec<_> = installed(&root).into_iter().map(|b| b.name).collect();
    left.sort();
    assert_eq!(left, vec!["c-release", "d-release", "e-release"]);
    assert_eq!(rollback(&root, None).unwrap(), "d-release");
    assert_eq!(std::fs::read_to_string(root.join("current.txt")).unwrap(), "d-release");
    assert_eq!(std::fs::read_to_string(root.join("previous.txt")).unwrap(), "e-release");
    assert!(rollback(&root, Some("nope")).is_err());
    assert_eq!(std::fs::read_to_string(root.join("current.txt")).unwrap(), "d-release");
    assert!(installed(&root).iter().any(|b| b.name == "d-release" && b.is_current));
}
