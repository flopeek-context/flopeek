use super::*;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("flopeek-module-resolution-{suffix}"))
}

#[test]
fn resolves_jsonc_paths_and_base_url_with_deterministic_precedence() {
    let root = temp_root();
    fs::create_dir_all(root.join("src/components")).expect("mkdir");
    fs::write(
        root.join("tsconfig.json"),
        r#"{
              // JSONC is intentional.
              "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                  "@app/*": ["src/*"],
                  "@app/components/*": ["missing/*", "src/components/*"],
                },
              },
            }"#,
    )
    .expect("config");
    fs::write(
        root.join("src/components/Button.tsx"),
        "export const Button = 1;",
    )
    .expect("source");
    let resolver = ModuleResolver::load(&root);
    let known = ["src/components/Button.tsx".to_string()]
        .into_iter()
        .collect();
    assert_eq!(
        resolver.resolve_non_relative("@app/components/Button", &known),
        NonRelativeResolution::Local("src/components/Button.tsx".to_string())
    );
    assert_eq!(resolver.basis.status, "complete");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn local_extends_preserves_base_paths_and_rejects_cycles() {
    let root = temp_root();
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(
        root.join("base.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@base/*":["src/*"]}}}"#,
    )
    .expect("base");
    fs::write(
        root.join("tsconfig.json"),
        r#"{"extends":"./base","compilerOptions":{"paths":{"@local/*":["src/*"]}}}"#,
    )
    .expect("config");
    fs::write(root.join("src/item.ts"), "export const item = 1;").expect("source");
    let resolver = ModuleResolver::load(&root);
    let known = ["src/item.ts".to_string()].into_iter().collect();
    assert_eq!(
        resolver.resolve_non_relative("@local/item", &known),
        NonRelativeResolution::Local("src/item.ts".to_string())
    );
    assert_eq!(
        resolver.resolve_non_relative("@base/item", &known),
        NonRelativeResolution::External
    );
    fs::write(root.join("base.json"), r#"{"extends":"./tsconfig"}"#).expect("cycle");
    let cycle = ModuleResolver::load(&root);
    assert_eq!(cycle.basis.status, "unavailable");
    assert!(cycle.basis.limitations[0].contains("tsconfig-extends-cycle"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn rejects_repository_escape_invalid_jsonc_and_package_extends() {
    let root = temp_root();
    fs::create_dir_all(&root).expect("mkdir");
    fs::write(root.join("tsconfig.json"), r#"{"extends":"../outside"}"#).expect("escape config");
    let escape = ModuleResolver::load(&root);
    assert_eq!(escape.basis.status, "unavailable");
    assert!(
        escape
            .basis
            .limitations
            .iter()
            .any(|reason| reason == "tsconfig-extends-escapes-repository")
    );

    fs::write(root.join("tsconfig.json"), r#"{"compilerOptions": {"#).expect("invalid config");
    let invalid = ModuleResolver::load(&root);
    assert_eq!(invalid.basis.status, "unavailable");
    assert_eq!(invalid.basis.config_files.len(), 1);
    assert!(
        invalid
            .basis
            .limitations
            .iter()
            .any(|reason| reason.starts_with("tsconfig-invalid-jsonc:"))
    );

    fs::write(root.join("tsconfig.json"), r#"{"extends":"shared-config"}"#)
        .expect("package config");
    let package = ModuleResolver::load(&root);
    assert_eq!(package.basis.status, "unavailable");
    assert!(
        package
            .basis
            .limitations
            .iter()
            .any(|reason| reason == "tsconfig-package-extends-unsupported")
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn enforces_config_file_bound_and_all_known_module_extensions() {
    let root = temp_root();
    fs::create_dir_all(&root).expect("mkdir");
    let mut parent = "base15".to_string();
    fs::write(root.join("base15.json"), r#"{"compilerOptions":{}}"#).expect("base");
    for index in (0..15).rev() {
        let current = format!("base{index}");
        fs::write(
            root.join(format!("{current}.json")),
            format!(r#"{{"extends":"./{parent}"}}"#),
        )
        .expect("chain");
        parent = current;
    }
    fs::write(
        root.join("tsconfig.json"),
        format!(r#"{{"extends":"./{parent}"}}"#),
    )
    .expect("root config");
    let capped = ModuleResolver::load(&root);
    assert_eq!(capped.basis.status, "truncated");
    assert!(
        capped
            .basis
            .limitations
            .iter()
            .any(|reason| reason.contains("tsconfig-files-capped"))
    );

    let declaration_path = ["src/types.d.ts".to_string()].into_iter().collect();
    assert_eq!(
        resolve_known_path("src/types", &declaration_path),
        Some("src/types.d.ts".to_string())
    );
    let index_path = ["src/components/index.tsx".to_string()]
        .into_iter()
        .collect();
    assert_eq!(
        resolve_known_path("src/components", &index_path),
        Some("src/components/index.tsx".to_string())
    );
    fs::remove_dir_all(root).expect("cleanup");
}
