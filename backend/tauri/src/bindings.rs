use std::path::Path;

pub fn generate() -> Result<String, Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("bindings.ts");
    crate::commands::builder::<tauri::Wry>()
        .export(specta_typescript::Typescript::default(), &path)?;
    let source = std::fs::read_to_string(path)?;
    adapt_transport(&source)
}

fn adapt_transport(source: &str) -> Result<String, Box<dyn std::error::Error>> {
    let original = "import { invoke as __TAURI_INVOKE } from \"@tauri-apps/api/core\";";
    if source.matches(original).count() != 1 {
        return Err("Specta invoke import changed; update the transport adapter".into());
    }
    let source = source.replacen(
        original,
        "import { invoke as __TAURI_INVOKE } from \"./transport\";",
        1,
    );
    let source = source
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("{}\n", source.trim_end()))
}

pub fn export(path: &Path, check: bool) -> Result<(), Box<dyn std::error::Error>> {
    let generated = generate()?;
    if check {
        if std::fs::read_to_string(path)?.replace("\r\n", "\n") != generated.replace("\r\n", "\n") {
            return Err("Generated bindings are stale; run pnpm bindings:generate".into());
        }
    } else {
        std::fs::write(path, generated)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn transport_replacement_requires_exactly_one_import() {
        assert!(super::adapt_transport("").is_err());
        let import = "import { invoke as __TAURI_INVOKE } from \"@tauri-apps/api/core\";";
        assert!(super::adapt_transport(&format!("{import}\n{import}")).is_err());
        assert!(super::adapt_transport(import)
            .unwrap()
            .contains("./transport"));
    }
}
