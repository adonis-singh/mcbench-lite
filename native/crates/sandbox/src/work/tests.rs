//! The bridge's tests: `sandbox-symlink.test.js` (4) and the bridge's part
//! of `sandbox-errors.test.js` (3).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;

struct Root(PathBuf);

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn root(label: &str) -> Root {
    let path = std::env::temp_dir().join(format!("mcbench-work-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(path.join("outside")).unwrap();
    std::fs::write(path.join("outside/secret.txt"), "host secret").unwrap();
    Root(std::fs::canonicalize(path).unwrap())
}

fn chat(root: &Root, id: &str) -> Workspace {
    Workspace {
        chat: id.to_string(),
        host: root.0.join("data/sandbox").join(id),
    }
}

fn link(target: impl AsRef<Path>, at: impl AsRef<Path>) {
    std::os::unix::fs::symlink(target, at).unwrap();
}

fn relative(from: &Path, to: &Path) -> PathBuf {
    let common = from
        .components()
        .zip(to.components())
        .take_while(|(a, b)| a == b)
        .count();
    let mut out = PathBuf::new();
    for _ in from.components().skip(common) {
        out.push("..");
    }
    for part in to.components().skip(common) {
        out.push(part);
    }
    out
}

/// `workspace(id)` of the Node test: a /work with links out of it.
fn workspace(root: &Root, id: &str) -> Workspace {
    let ws = chat(root, id);
    let work = work_dir(&ws).unwrap();
    let outside = root.0.join("outside");
    let secret = outside.join("secret.txt");
    std::fs::write(work.join("data.txt"), "inside").unwrap();
    link(&secret, work.join("absolute"));
    link(relative(&work, &secret), work.join("relative"));
    link(&outside, work.join("dir"));
    link(relative(&work, &outside), work.join("reldir"));
    link(outside.join("missing.txt"), work.join("dangling"));
    link("data.txt", work.join("inner"));
    ws
}

fn snapshot(root: &Root) -> BTreeMap<String, String> {
    std::fs::read_dir(root.0.join("outside"))
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().to_string_lossy().into_owned(),
                std::fs::read_to_string(e.path()).unwrap_or_default(),
            )
        })
        .collect()
}

// sandbox-symlink.test.js: "host reads refuse absolute, relative, directory
// and dangling symlinks out of /work"
#[test]
fn host_reads_refuse_absolute_relative_directory_and_dangling_symlinks_out_of_work() {
    let root = root("reads");
    let ws = workspace(&root, "reads");
    for name in [
        "absolute",
        "relative",
        "dir/secret.txt",
        "reldir/secret.txt",
        "dangling",
        "/work/absolute",
        "../outside/secret.txt",
    ] {
        let error = read_file_for_build(&ws, name).unwrap_err();
        assert!(error.contains("escapes /work"), "{name}: {error}");
    }
    assert_eq!(read_file_for_build(&ws, "data.txt").unwrap(), "inside");
    assert_eq!(read_file_for_build(&ws, "inner").unwrap(), "inside");
    assert!(
        read_file_for_build(&ws, "nothing.txt")
            .unwrap_err()
            .contains("No such file in /work")
    );
    assert!(
        resolve_work_path(&ws, "dir/new.txt", false)
            .unwrap_err()
            .contains("escapes /work")
    );
}

// sandbox-symlink.test.js: "host writes never follow or replace symlinks
// and leave outside files untouched"
#[test]
fn host_writes_never_follow_or_replace_symlinks_and_leave_outside_files_untouched() {
    let root = root("writes");
    let ws = workspace(&root, "writes");
    let before = snapshot(&root);
    for name in ["absolute", "relative", "dangling", "inner"] {
        let error = write_work_file(&ws, name, b"overwritten").unwrap_err();
        assert!(error.contains("symlink"), "{name}: {error}");
    }
    for name in [
        "dir/secret.txt",
        "dir/created.txt",
        "reldir/created.txt",
        "dir/nested/created.txt",
    ] {
        let error = write_work_file(&ws, name, b"overwritten").unwrap_err();
        assert!(error.contains("escapes /work"), "{name}: {error}");
    }
    assert_eq!(snapshot(&root), before);
    assert_eq!(
        std::fs::read_to_string(ws.host.join("data.txt")).unwrap(),
        "inside"
    );
    write_work_file(&ws, "nested/out.txt", b"ok").unwrap();
    assert_eq!(
        std::fs::read_to_string(ws.host.join("nested/out.txt")).unwrap(),
        "ok"
    );
    write_work_file(&ws, "data.txt", b"new").unwrap();
    assert_eq!(
        std::fs::read_to_string(ws.host.join("data.txt")).unwrap(),
        "new"
    );
}

// sandbox-symlink.test.js: "an opened descriptor for a different inode
// cannot disclose or truncate an outside file"
#[test]
fn an_opened_descriptor_for_a_different_inode_cannot_disclose_or_truncate_an_outside_file() {
    let root = root("swap");
    let ws = workspace(&root, "descriptor-swap");
    let target = std::fs::canonicalize(ws.host.join("data.txt")).unwrap();
    let secret = root.0.join("outside/secret.txt");
    super::tests_hook::redirect(target, secret.clone());
    assert!(
        read_work_file(&ws, "data.txt", None)
            .unwrap_err()
            .contains("escapes /work")
    );
    assert!(
        write_work_file(&ws, "data.txt", b"overwritten")
            .unwrap_err()
            .contains("escapes /work")
    );
    super::tests_hook::clear();
    assert_eq!(std::fs::read_to_string(&secret).unwrap(), "host secret");
}

// sandbox-symlink.test.js: "mirrored uploads and archived reports are not
// written through symlinks"
#[test]
fn mirrored_uploads_and_archived_reports_are_not_written_through_symlinks() {
    let root = root("mirror");
    let ws = workspace(&root, "mirror");
    let secret = root.0.join("outside/secret.txt");
    let images = root.0.join("store-images/mirror");
    std::fs::create_dir_all(&images).unwrap();
    std::fs::write(images.join("shot.png"), "png").unwrap();
    link(&secret, ws.host.join("images/shot.png"));
    let before = snapshot(&root);
    sync_images(&ws, &root.0.join("store-images")).unwrap();
    assert_eq!(snapshot(&root), before);
    std::fs::remove_dir_all(ws.host.join("attachments")).unwrap();
    link(root.0.join("outside"), ws.host.join("attachments"));
    let attachments = root.0.join("store-attachments/mirror");
    std::fs::create_dir_all(&attachments).unwrap();
    std::fs::write(attachments.join("upload.txt"), "upload").unwrap();
    assert!(
        sync_attachments(&ws, &root.0.join("store-attachments"))
            .unwrap_err()
            .contains("escapes /work")
    );
    assert_eq!(snapshot(&root), before);
    link(root.0.join("outside"), ws.host.join("verification"));
    assert!(
        copy_into_work(
            &ws,
            "verification/report.json",
            &images.join("shot.png"),
            true
        )
        .unwrap_err()
        .contains("escapes /work")
    );
    assert_eq!(snapshot(&root), before);
}

// sandbox-errors.test.js: "the host-side /work functions tell a file error
// by its /work path and the meaning of its code"
#[test]
fn the_host_side_work_functions_tell_a_file_error_by_its_work_path_and_the_meaning_of_its_code() {
    let root = root("errors");
    let ws = chat(&root, "errors");
    std::fs::write(work_dir(&ws).unwrap().join("f.txt"), "a file").unwrap();
    let source = root.0.join("outside/secret.txt");
    let cases: Vec<String> = vec![
        read_work_file(&ws, "f.txt/g", None).unwrap_err(),
        read_file_for_build(&ws, "/work/f.txt/g").unwrap_err(),
        write_work_file(&ws, "f.txt/g", b"x").unwrap_err(),
        write_work_file(&ws, "f.txt/g/h", b"x").unwrap_err(),
        resolve_work_path(&ws, "f.txt/g", true).unwrap_err(),
        work_file_exists(&ws, "f.txt/g").unwrap_err(),
        copy_into_work(&ws, "f.txt/g", &source, true).unwrap_err(),
    ];
    let host = root.0.to_string_lossy().into_owned();
    for (index, error) in cases.iter().enumerate() {
        assert!(
            !error.contains(&host),
            "case {index} names the data folder: {error}"
        );
        assert!(
            error.ends_with("/work/f.txt/g: not a directory (ENOTDIR)")
                || error.ends_with("/work/f.txt/g/h: not a directory (ENOTDIR)"),
            "case {index}: {error}"
        );
    }
    assert_eq!(
        write_work_file(&ws, "images", b"x").unwrap_err(),
        "/work/images: illegal operation on a directory (EISDIR)"
    );
    // The bridge's own errors already name only the /work path.
    assert_eq!(
        read_work_file(&ws, "missing.txt", None).unwrap_err(),
        "No such file in /work: missing.txt"
    );
}

// sandbox-errors.test.js: "a file where images/ should be is told as
// /work/images"
#[test]
fn a_file_where_images_should_be_is_told_as_work_images() {
    let root = root("images");
    let ws = chat(&root, "images");
    let work = work_dir(&ws).unwrap();
    std::fs::remove_dir_all(work.join("images")).unwrap();
    std::fs::write(work.join("images"), "not a folder").unwrap();
    assert_eq!(
        work_dir(&ws).unwrap_err(),
        "/work/images: file already exists (EEXIST)"
    );
}

// sandbox-errors.test.js: "no tool result built from a file error contains
// the data folder path": the bridge calls that `build`, `view_image`,
// `test_build` and `bash` make (the tools' envelopes are mcbench-tools').
#[test]
fn no_bridge_error_a_tool_returns_contains_the_data_folder_path() {
    let root = root("tools");
    let ws = chat(&root, "tools");
    let work = work_dir(&ws).unwrap();
    std::fs::write(work.join("f.txt"), "a file").unwrap();
    std::fs::write(work.join("verification"), "not a folder").unwrap();
    let mut errors = vec![
        read_file_for_build(&ws, "f.txt/g").unwrap_err(),
        write_work_file(&ws, "f.txt/g", b"x").unwrap_err(),
        resolve_work_path(&ws, "f.txt/g", false).unwrap_err(),
        copy_into_work(
            &ws,
            "verification/report.json",
            &root.0.join("outside/secret.txt"),
            true,
        )
        .unwrap_err(),
    ];
    // bash mirrors uploads into /work/images first, which a model can make a file.
    std::fs::remove_dir_all(work.join("images")).unwrap();
    std::fs::write(work.join("images"), "not a folder").unwrap();
    let images = root.0.join("store-images");
    std::fs::create_dir_all(images.join("tools")).unwrap();
    std::fs::write(images.join("tools/shot.png"), "png").unwrap();
    errors.push(sync_images(&ws, &images).unwrap_err());
    errors.push(work_dir(&ws).unwrap_err());
    let host = root.0.to_string_lossy().into_owned();
    for error in &errors {
        assert!(!error.contains(&host), "{error}");
    }
    assert!(errors[0].contains("/work/f.txt/g: not a directory (ENOTDIR)"));
    assert!(
        errors[3].contains("/work/verification/report.json: not a directory (ENOTDIR)"),
        "{}",
        errors[3]
    );
    assert_eq!(errors[4], "/work/images: file already exists (EEXIST)");
    assert_eq!(errors[5], "/work/images: file already exists (EEXIST)");
}

#[test]
fn work_paths_drop_a_leading_work() {
    assert_eq!(work_path("f.txt"), "/work/f.txt");
    assert_eq!(work_path("/work/f.txt"), "/work/f.txt");
    assert_eq!(work_path("work/f.txt"), "/work/f.txt");
    assert_eq!(work_path("/work"), "/work/");
    assert_eq!(work_path("workshop/a"), "/work/workshop/a");
    assert_eq!(work_path("/etc/passwd"), "/work//etc/passwd");
}
