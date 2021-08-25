use std::{ops::Index, path::PathBuf};

use base_db::{CrateName, FileId, ProcMacro};
use cfg::CfgDiff;
use paths::AbsPath;
use rustc_hash::FxHashMap;

use crate::{CargoWorkspace, Sysroot, WorkspaceBuildScripts, cfg_flag::CfgFlag, project_json::{CrateData, DepData, EditionData, ProjectJsonData}, sysroot::SysrootCrate};

pub type CfgOverrides = FxHashMap<String, CfgDiff>;

pub fn cargo_to_json(
    rustc_cfg: Vec<CfgFlag>,
    override_cfg: &CfgOverrides,
    load_proc_macro: &mut dyn FnMut(&AbsPath) -> Vec<ProcMacro>,
    load: &mut dyn FnMut(&AbsPath) -> Option<FileId>,
    cargo: &CargoWorkspace,
    build_scripts: &WorkspaceBuildScripts,
    sysroot: Option<&Sysroot>,
    rustc: &Option<CargoWorkspace>,
) -> ProjectJsonData {
    let _p = profile::span("cargo_to_crate_graph");
    let crates = cargo
        .packages()
        .map(|id| &cargo[id])
        .map(|pkg| {
            let _deps = 
                &pkg.dependencies
                    .iter()
                    .collect::<Vec<_>>();
            CrateData {
                display_name: Some(pkg.name.to_owned()),
                root_module: PathBuf::new(),
                edition: EditionData::from(pkg.edition),
                deps: Vec::new(),
                cfg: Vec::new(),
                target: None,
                env: FxHashMap::default(),
                proc_macro_dylib_path: None,
                is_workspace_member: None,
                source: None,
                is_proc_macro: false,
            }
        })
        .collect();
    let json = ProjectJsonData { sysroot_src: None, crates: crates };
    json
}
