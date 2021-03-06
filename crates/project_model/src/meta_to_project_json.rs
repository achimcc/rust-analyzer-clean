use std::path::PathBuf;

use base_db::CrateName;
use cargo_metadata::PackageId;
use rustc_hash::FxHashMap;

use crate::{
    cargo_workspace::DepKind,
    project_json::{CrateData, DepData, EditionData, ProjectJsonData},
};

pub fn meta_to_json(mut meta: cargo_metadata::Metadata) -> ProjectJsonData {
    let mut pkg_by_id: FxHashMap<PackageId, usize> = FxHashMap::default();
    let _p = profile::span("meta_to_json");
    meta.packages.sort_by(|a, b| a.id.cmp(&b.id));
    let mut crates: Vec<CrateData> = Vec::new();
    for pkg in &mut meta.packages.into_iter() {
        let root_id = crates.len();
        pkg_by_id.insert(pkg.id, root_id);
        for tgt in &mut pkg.targets.iter() {
            let edition = tgt.edition.parse::<EditionData>().unwrap_or_else(|err| {
                log::error!("Failed to parse edition {:?}", err);
                EditionData::Edition2018
            });
            let target = tgt.kind.first();
            let target = match target {
                Some(target) => Some(String::from(target.as_str())),
                None => None,
            };
            let data = CrateData {
                display_name: Some(tgt.name.to_owned()),
                root_module: PathBuf::from(&tgt.src_path),
                edition,
                deps: Vec::new(), // resolved further down
                cfg: Vec::new(),
                target,
                env: FxHashMap::default(),
                proc_macro_dylib_path: None,
                is_workspace_member: None,
                source: None,
                is_proc_macro: false,
            };
            crates.push(data);
            if crates.len() + 1 > root_id {
                // Should not push root to its own deps!
                let name = CrateName::normalize_dashes(&tgt.name);
                let dep = DepData { krate: crates.len(), name };
                crates[root_id].deps.push(dep);
            }
        }
    }
    let resolve = meta.resolve.expect("metadata executed with deps");
    for mut node in resolve.nodes {
        let source = match pkg_by_id.get(&node.id) {
            Some(&src) => src,
            // FIXME: replace this and a similar branch below with `.unwrap`, once
            // https://github.com/rust-lang/cargo/issues/7841
            // is fixed and hits stable (around 1.43-is probably?).
            None => {
                log::error!("Node id do not match in cargo metadata, ignoring {}", node.id);
                continue;
            }
        };
        node.deps.sort_by(|a, b| a.pkg.cmp(&b.pkg));
        for (dep_node, _kind) in node
            .deps
            .iter()
            .flat_map(|dep| DepKind::iter(&dep.dep_kinds).map(move |kind| (dep, kind)))
        {
            let pkg = match pkg_by_id.get(&dep_node.pkg) {
                Some(&pkg) => pkg,
                None => {
                    log::error!(
                        "Dep node id do not match in cargo metadata, ignoring {}",
                        dep_node.pkg
                    );
                    continue;
                }
            };
            let name = CrateName::new(&dep_node.name).unwrap();
            let dep = DepData { krate: pkg, name };
            crates[source].deps.push(dep);
        }
        //  crates[source].active_features.extend(node.features);
    }
    let json = ProjectJsonData { sysroot_src: None, crates: crates.to_vec() };
    json
}
