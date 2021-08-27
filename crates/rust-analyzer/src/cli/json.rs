//! Read project from Cargo.toml convert to rust-project.json

use project_model::{ManifestPath, ProjectJson, ProjectManifest, ProjectWorkspace};

use std::convert::TryFrom;
use std::path::Path;

use anyhow::Result;
use project_model::{meta_to_json, CargoConfig, CargoWorkspace};
use vfs::AbsPathBuf;

use crate::cli::flags;

impl flags::Json {
    pub fn run(self) -> anyhow::Result<()> {
        let cargo_config = Default::default();
        let _p = profile::span("json");
        //   let root = self.path;
        //   let root = AbsPathBuf::assert(std::env::current_dir()?.join(root));
        //  let root = ProjectManifest::discover_single(&root)?;
        //  let workspace = ProjectWorkspace::load(root, &cargo_config, &|_|{})?;
        //  println!("es: {:?}", workspace);
        let _res = load_workspace_at(&self.path, &cargo_config, &|_| {});
        Ok(())
    }
}

fn load_workspace_at(
    root: &Path,
    cargo_config: &CargoConfig,
    progress: &dyn Fn(String),
) -> Result<()> {
    let root = AbsPathBuf::assert(std::env::current_dir()?.join(root));
    let cargo_toml = ManifestPath::try_from(root).unwrap();
    let meta = CargoWorkspace::fetch_metadata(&cargo_toml, cargo_config, progress)?;
   // let json = serde_json::to_string(&meta).expect("serialization of crate_graph must work");
   // println!("{:}", json);
    let json = meta_to_json(meta);
    println!("{:?}", json);
    //    let project = ProjectJson::new(&cargo_toml.parent().to_path_buf(), json);
    //    println!("success: {:?}", project);
    Ok(())
}
