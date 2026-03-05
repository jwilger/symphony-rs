use std::path::PathBuf;
use std::process::Command;

#[test]
#[ignore = "enabled during cargo-mutants runs via .cargo/mutants.toml (--include-ignored)"]
fn playwright_mutation_smoke_suite() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .expect("crate layout should contain repository root")
        .to_path_buf();
    let e2e_dir = repo_root.join("e2e");
    let node_modules = e2e_dir.join("node_modules");
    let app_binary = PathBuf::from(env!("CARGO_BIN_EXE_symphony-app"));

    if !node_modules.exists() {
        let install = Command::new("bun")
            .args(["install", "--frozen-lockfile"])
            .current_dir(&e2e_dir)
            .status()
            .expect("failed to run bun install for e2e dependencies");
        assert!(
            install.success(),
            "bun install failed with status={install}"
        );
    }

    let status = Command::new("bun")
        .args([
            "run",
            "test:e2e",
            "tests/mutants-smoke.spec.ts",
            "--reporter=line",
            "--workers=1",
            "--max-failures=1",
        ])
        .current_dir(&e2e_dir)
        .env("CI", "1")
        .env("SYMPHONY_APP_BIN", &app_binary)
        .env("SYMPHONY_HARNESS_STARTUP_TIMEOUT_MS", "20000")
        .status()
        .expect("failed to execute playwright mutant smoke suite");

    assert!(
        status.success(),
        "playwright mutant smoke suite failed with status={status}"
    );
}
