use anyhow::{Context, Result};
use std::fs::{self, File, Permissions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/dracut_templates.rs"));

pub(crate) const MODULE_DIR_PRIMARY: &str = "/usr/lib/dracut/modules.d/90zfs-beskar";
pub(crate) const MODULE_DIR_FALLBACK: &str = "/lib/dracut/modules.d/90zfs-beskar";
pub(crate) const BESKAR_TOKEN_LABEL: &str = "BESKARKEY";
pub(crate) const SETUP_NAME: &str = "module-setup.sh";
pub(crate) const DEFAULT_MOUNTPOINT: &str = "/run/beskar";

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub(crate) struct ModuleContext<'a> {
    pub mountpoint: &'a str,
    pub unit_name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ExpectedModule {
    pub mount_unit: String,
    pub setup: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ModulePaths {
    pub root: PathBuf,
    pub mount_unit: PathBuf,
    pub setup: PathBuf,
}

impl ModulePaths {
    pub fn new<P: AsRef<Path>>(root: P, unit_name: &str) -> Self {
        let root = root.as_ref().to_path_buf();
        Self {
            mount_unit: root.join(unit_name),
            setup: root.join(SETUP_NAME),
            root,
        }
    }
}

pub(crate) fn module_dir_candidates() -> [PathBuf; 2] {
    [
        PathBuf::from(MODULE_DIR_PRIMARY),
        PathBuf::from(MODULE_DIR_FALLBACK),
    ]
}

pub(crate) fn preferred_module_dir() -> PathBuf {
    let candidates = module_dir_candidates();

    for candidate in &candidates {
        if candidate.exists() {
            return candidate.clone();
        }
    }

    for candidate in &candidates {
        if candidate
            .parent()
            .map(|parent| parent.exists())
            .unwrap_or(false)
        {
            return candidate.clone();
        }
    }

    candidates
        .into_iter()
        .next()
        .unwrap_or_else(|| PathBuf::from(MODULE_DIR_PRIMARY))
}

pub(crate) fn expected_module(ctx: &ModuleContext<'_>) -> ExpectedModule {
    let replacements = replacements(ctx);
    ExpectedModule {
        mount_unit: render_template(MOUNT_UNIT_TEMPLATE, &replacements),
        setup: render_template(MODULE_SETUP_TEMPLATE, &replacements),
    }
}

pub(crate) fn module_is_current(
    module_paths: &ModulePaths,
    ctx: &ModuleContext<'_>,
) -> Result<bool> {
    let expected = expected_module(ctx);

    if !module_paths.mount_unit.exists() || !module_paths.setup.exists() {
        return Ok(false);
    }

    let mount_unit = fs::read_to_string(&module_paths.mount_unit)
        .with_context(|| format!("read {}", module_paths.mount_unit.display()))?;
    let setup = fs::read_to_string(&module_paths.setup)
        .with_context(|| format!("read {}", module_paths.setup.display()))?;

    Ok(mount_unit == expected.mount_unit && setup == expected.setup)
}

pub(crate) fn install_module(module_paths: &ModulePaths, ctx: &ModuleContext<'_>) -> Result<()> {
    fs::create_dir_all(&module_paths.root).with_context(|| {
        format!(
            "create dracut module directory {}",
            module_paths.root.display()
        )
    })?;

    let expected = expected_module(ctx);

    write_file(&module_paths.mount_unit, &expected.mount_unit, 0o644)?;
    write_file(&module_paths.setup, &expected.setup, 0o750)?;

    Ok(())
}

fn replacements(ctx: &ModuleContext<'_>) -> Vec<(&'static str, String)> {
    vec![
        ("VERSION", VERSION.to_string()),
        ("TOKEN_LABEL", BESKAR_TOKEN_LABEL.to_string()),
        ("MOUNTPOINT", ctx.mountpoint.to_string()),
        ("MOUNT_UNIT_NAME", ctx.unit_name.clone()),
    ]
}

pub(crate) fn derive_mount_unit_name(mountpoint: &str) -> String {
    let trimmed = mountpoint.trim_start_matches('/');
    if trimmed.is_empty() {
        return String::from("-.mount");
    }
    let mut escaped = String::new();
    for ch in trimmed.chars() {
        match ch {
            '/' => escaped.push('-'),
            '-' => escaped.push_str("\\x2d"),
            _ => escaped.push(ch),
        }
    }
    escaped.push_str(".mount");
    escaped
}

fn render_template(template: &str, replacements: &[(&str, String)]) -> String {
    let mut rendered = template.to_string();
    for (key, value) in replacements {
        let needle = format!("{{{{{}}}}}", key);
        rendered = rendered.replace(&needle, value);
    }
    rendered
}

fn write_file(path: &Path, contents: &str, mode: u32) -> Result<()> {
    let mut file = File::create(path).with_context(|| format!("create {}", path.display()))?;
    file.write_all(contents.as_bytes())?;
    file.sync_all().ok();
    fs::set_permissions(path, Permissions::from_mode(mode))
        .with_context(|| format!("set permissions on {}", path.display()))?;
    Ok(())
}
