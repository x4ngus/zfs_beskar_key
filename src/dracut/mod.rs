use anyhow::{Context, Result};
use std::fs::{self, File, Permissions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/dracut_templates.rs"));

pub(crate) const MODULE_DIR_PRIMARY: &str = "/usr/lib/dracut/modules.d/90zfs-beskar";
pub(crate) const MODULE_DIR_FALLBACK: &str = "/lib/dracut/modules.d/90zfs-beskar";
pub(crate) const BESKAR_TOKEN_LABEL: &str = "BESKARKEY";
pub(crate) const SCRIPT_NAME: &str = "beskar-load-key.sh";
pub(crate) const SERVICE_NAME: &str = "beskar-load-key.service";
pub(crate) const DROPIN_DIR: &str = "zfs-load-key.service.d";
pub(crate) const DROPIN_NAME: &str = "beskar.conf";
pub(crate) const SETUP_NAME: &str = "module-setup.sh";
pub(crate) const DEFAULT_MOUNTPOINT: &str = "/run/beskar";

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub(crate) struct ModuleContext<'a> {
    pub mountpoint: &'a str,
    pub key_path: &'a str,
    pub key_sha256: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub(crate) struct ExpectedModule {
    pub script: String,
    pub service: String,
    pub dropin: String,
    pub setup: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ModulePaths {
    pub root: PathBuf,
    pub script: PathBuf,
    pub service: PathBuf,
    pub dropin_dir: PathBuf,
    pub dropin: PathBuf,
    pub setup: PathBuf,
}

impl ModulePaths {
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        let root = root.as_ref().to_path_buf();
        let dropin_dir = root.join(DROPIN_DIR);
        Self {
            script: root.join(SCRIPT_NAME),
            service: root.join(SERVICE_NAME),
            dropin_dir,
            dropin: root.join(DROPIN_DIR).join(DROPIN_NAME),
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
        script: render_template(SCRIPT_TEMPLATE, &replacements),
        service: render_template(SERVICE_TEMPLATE, &replacements),
        dropin: render_template(DROPIN_TEMPLATE, &replacements),
        setup: render_template(MODULE_SETUP_TEMPLATE, &replacements),
    }
}

pub(crate) fn module_is_current(
    module_paths: &ModulePaths,
    ctx: &ModuleContext<'_>,
) -> Result<bool> {
    let expected = expected_module(ctx);

    if !module_paths.script.exists()
        || !module_paths.service.exists()
        || !module_paths.dropin.exists()
        || !module_paths.setup.exists()
    {
        return Ok(false);
    }

    let script = fs::read_to_string(&module_paths.script)
        .with_context(|| format!("read {}", module_paths.script.display()))?;
    let service = fs::read_to_string(&module_paths.service)
        .with_context(|| format!("read {}", module_paths.service.display()))?;
    let dropin = fs::read_to_string(&module_paths.dropin)
        .with_context(|| format!("read {}", module_paths.dropin.display()))?;
    let setup = fs::read_to_string(&module_paths.setup)
        .with_context(|| format!("read {}", module_paths.setup.display()))?;

    Ok(script == expected.script
        && service == expected.service
        && dropin == expected.dropin
        && setup == expected.setup)
}

pub(crate) fn install_module(module_paths: &ModulePaths, ctx: &ModuleContext<'_>) -> Result<()> {
    fs::create_dir_all(&module_paths.root).with_context(|| {
        format!(
            "create dracut module directory {}",
            module_paths.root.display()
        )
    })?;

    let expected = expected_module(ctx);

    write_file(&module_paths.script, &expected.script, 0o750)?;
    write_file(&module_paths.service, &expected.service, 0o644)?;
    fs::create_dir_all(&module_paths.dropin_dir).with_context(|| {
        format!(
            "create drop-in directory {}",
            module_paths.dropin_dir.display()
        )
    })?;
    write_file(&module_paths.dropin, &expected.dropin, 0o644)?;
    write_file(&module_paths.setup, &expected.setup, 0o750)?;

    Ok(())
}

fn replacements(ctx: &ModuleContext<'_>) -> Vec<(&'static str, String)> {
    vec![
        ("VERSION", VERSION.to_string()),
        ("TOKEN_LABEL", BESKAR_TOKEN_LABEL.to_string()),
        ("MOUNTPOINT", ctx.mountpoint.to_string()),
        ("SCRIPT_NAME", SCRIPT_NAME.to_string()),
        ("SERVICE_NAME", SERVICE_NAME.to_string()),
        ("DROPIN_DIR", DROPIN_DIR.to_string()),
        ("DROPIN_NAME", DROPIN_NAME.to_string()),
        ("KEY_PATH", ctx.key_path.to_string()),
        (
            "KEY_SHA256",
            ctx.key_sha256
                .map(|s| s.to_string())
                .unwrap_or_else(String::new),
        ),
    ]
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
