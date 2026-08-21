//! The cross-snapshot semantic cache's invalidation contract
//! (`docs/design/compiler-core.md` §11, TASK-126):
//!
//! - an unchanged file's semantics are reused across snapshots;
//! - a dependency's **body-only** change keeps its importers' entries;
//! - a dependency's **exported-declaration** change invalidates exactly
//!   the importers.
//!
//! The cache is observable with or without a TypeScript toolchain: since
//! TASK-124 a missing backend degrades the typed facts instead of failing
//! the pass, so `check()` runs — and counts cache hits — either way.

use std::fs;
use std::path::PathBuf;

use rlc::engine::{CheckRequest, Engine, ProjectOptions};

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rl-cache-{}-{tag}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn body_changes_keep_importers_and_export_changes_invalidate_them() {
    let dir = tmpdir("invalidate");
    let shared = dir.join("shared.rl");
    let user = dir.join("user.rl");
    fs::write(
        &shared,
        "export enum Token { Num(value: number), Eof }\nexport const noise = 1;\n",
    )
    .unwrap();
    fs::write(
        &user,
        "import { Token } from \"./shared.rl\";\n\
         export function f(t: Token): number {\n\
         \x20 return match (t) { Num(value) => value, Eof => 0 };\n\
         }\n",
    )
    .unwrap();

    let engine = Engine::new(None);
    let mut project = engine
        .open_project(
            &[dir.to_string_lossy().to_string()],
            &ProjectOptions::default(),
        )
        .expect("opens without a toolchain (TASK-124)");
    let files = project.initial_files();

    // First pass: everything computes, nothing hits.
    let snapshot = project.update(&files).unwrap();
    project.check(&snapshot, &CheckRequest::default()).unwrap();
    assert_eq!(project.semantic_cache_hits(), 0);

    // Same content: both files hit.
    let snapshot = project.update(&files).unwrap();
    project.check(&snapshot, &CheckRequest::default()).unwrap();
    assert_eq!(project.semantic_cache_hits(), 2);

    // A body-only change to the dependency: shared recomputes (content
    // changed), the importer's key — its own content plus shared's
    // *exports* — is untouched, so user.rl hits.
    fs::write(
        &shared,
        "export enum Token { Num(value: number), Eof }\nexport const noise = 2;\n",
    )
    .unwrap();
    let snapshot = project.update(&files).unwrap();
    project.check(&snapshot, &CheckRequest::default()).unwrap();
    assert_eq!(
        project.semantic_cache_hits(),
        3,
        "the importer survived a dependency body change"
    );

    // An exported-declaration change: the importer's externs differ, so
    // user.rl recomputes too — no stale exhaustiveness model survives.
    fs::write(
        &shared,
        "export enum Token { Num(value: number), Word(text: string), Eof }\nexport const noise = 2;\n",
    )
    .unwrap();
    let snapshot = project.update(&files).unwrap();
    project.check(&snapshot, &CheckRequest::default()).unwrap();
    assert_eq!(
        project.semantic_cache_hits(),
        3,
        "an exported-declaration change invalidated the importer"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_unchanged_projection_is_shared_across_snapshots() {
    let dir = tmpdir("projection");
    let file = dir.join("a.rl");
    fs::write(&file, "export enum E { A(x: number), B }\n").unwrap();

    let engine = Engine::new(None);
    let mut project = engine
        .open_project(
            &[dir.to_string_lossy().to_string()],
            &ProjectOptions::default(),
        )
        .unwrap();
    let files = project.initial_files();
    let first = project.update(&files).unwrap();
    let second = project.update(&files).unwrap();
    // Same content ⇒ the same Arc — the projection was not recomputed.
    assert!(std::sync::Arc::ptr_eq(
        &first.files()[0],
        &second.files()[0]
    ));
    fs::remove_dir_all(&dir).ok();
}
