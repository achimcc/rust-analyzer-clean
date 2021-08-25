//! Read project from Cargo.toml convert to rust-project.json

use project_model::ManifestPath;

use std::convert::TryFrom;
use std::path::Path;

use anyhow::Result;
use project_model::{CargoConfig, CargoWorkspace, meta_to_json};
use vfs::AbsPathBuf;

use crate::cli::flags;

impl flags::Json {
    pub fn run(self) -> anyhow::Result<()> {
        let cargo_config = Default::default();
        let _p = profile::span("json");
        let _res = load_workspace_at(&self.path, &cargo_config, &|_|{});
        Ok(())
    }
}

fn load_workspace_at(
    root: &Path,
    cargo_config: &CargoConfig,
    progress: &dyn Fn(String),
) -> Result<()> {
    println!("begin!");
    let root = AbsPathBuf::assert(std::env::current_dir()?.join(root));
    println!("AbsPathBuf: {:?}", root);
    let root = ManifestPath::try_from(root).unwrap(); 
    let meta = CargoWorkspace::fetch_metadata(&root, cargo_config, progress)?;
    println!("starting conversion!");
    let json = meta_to_json(meta);
    println!("success: {:?}", json);
    Ok(())
}
