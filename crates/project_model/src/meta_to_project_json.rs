use std::path::PathBuf;

use base_db::CrateName;
use cargo_metadata::PackageId;
use rustc_hash::FxHashMap;

use crate::{cargo_workspace::DepKind, project_json::{CrateData, DepData, EditionData, ProjectJsonData}};

pub fn meta_to_json(mut meta: cargo_metadata::Metadata) -> ProjectJsonData {
    let mut pkg_by_id: FxHashMap<PackageId, usize> = FxHashMap::default();
    let _p = profile::span("meta_to_json");
    meta.packages.sort_by(|a, b| a.id.cmp(&b.id));
    meta.packages.iter().enumerate().for_each(|(id, pkg)| {
        pkg_by_id.insert(pkg.id.clone(), id);
    });
    let mut crates = &mut meta
        .packages
        .iter()
        .map(|pkg| {
            let edition = pkg.edition.parse::<EditionData>().unwrap_or_else(|err| {
                log::error!("Failed to parse edition {:?}", err);
                EditionData::Edition2018
            });
            CrateData {
                display_name: Some(pkg.name.to_owned()),
                root_module: PathBuf::new(),
                edition,
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
        .collect::<Vec<_>>();
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
