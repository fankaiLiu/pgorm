use crate::codegen::GeneratedFile;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub struct WriteOptions {
    pub dry_run: bool,
    pub check: bool,
}

#[derive(Debug, Default)]
pub struct WriteSummary {
    pub changed: Vec<PathBuf>,
    pub written: Vec<PathBuf>,
}

pub fn apply_generated_files(
    files: &[GeneratedFile],
    opts: WriteOptions,
) -> anyhow::Result<WriteSummary> {
    let mut files = files.to_vec();
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let mut summary = WriteSummary::default();

    for f in &files {
        let existing = std::fs::read_to_string(&f.path).ok();
        if existing.as_deref() != Some(f.content.as_str()) {
            summary.changed.push(f.path.clone());
        }
    }

    if opts.dry_run {
        for p in &summary.changed {
            println!("would write {}", p.display());
        }
        return Ok(summary);
    }

    if opts.check {
        if !summary.changed.is_empty() {
            anyhow::bail!("generated files are out of date");
        }
        return Ok(summary);
    }

    for f in &files {
        if !summary.changed.contains(&f.path) {
            continue;
        }
        write_atomic(&f.path, &f.content)?;
        summary.written.push(f.path.clone());
    }

    for p in &summary.written {
        println!("wrote {}", p.display());
    }

    Ok(summary)
}

fn write_atomic(path: &Path, content: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("failed to create directory {}: {e}", parent.display()))?;
    }

    let tmp = tmp_path(path);
    std::fs::write(&tmp, content)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        anyhow::anyhow!(
            "failed to rename {} -> {}: {e}",
            tmp.display(),
            path.display()
        )
    })?;
    Ok(())
}

fn tmp_path(path: &Path) -> PathBuf {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => path.with_extension(format!("{ext}.tmp")),
        None => path.with_extension("tmp"),
    }
}
