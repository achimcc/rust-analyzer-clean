use std::{collections::VecDeque, str::FromStr};

use base_db::{CrateDisplayName, CrateGraph, CrateId, CrateName, Edition, Env, FileId, ProcMacro};
use cfg::{CfgDiff, CfgOptions};
use paths::{AbsPath, AbsPathBuf};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::{
    build_scripts::BuildScriptOutput, cargo_workspace::DepKind, cfg_flag::CfgFlag,
    sysroot::SysrootCrate, CargoWorkspace, PackageData, Sysroot, TargetKind, WorkspaceBuildScripts,
};

pub type CfgOverrides = FxHashMap<String, CfgDiff>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CrateRoot {
    file_id: u32,
    edition: String,
    display_name: Option<String>,
    cfg_options: Vec<(String, Vec<String>)>,
    potential_cfg_options: Vec<(String, Vec<String>)>,
    env: Vec<(String, String)>,
    proc_macro: Vec<AbsPathBuf>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Dep {
    from: u32,
    name: String,
    to: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CrateGraphJson {
    roots: Vec<CrateRoot>,
    deps: Vec<Dep>,
}

impl Default for CrateGraphJson {
    fn default() -> Self {
        CrateGraphJson { roots: Vec::new(), deps: Vec::new() }
    }
}

impl CrateGraphJson {
    fn add_dep(&mut self, dep: Dep) {
        self.deps.push(dep);
    }

    fn add_crate_root(
        &mut self,
        file_id: FileId,
        edition: Edition,
        display_name: Option<CrateDisplayName>,
        cfg_options: CfgOptions,
        potential_cfg_options: CfgOptions,
        env: Env,
        _proc_macro: Vec<ProcMacro>,
    ) -> u32 {
        let env = env
            .iter()
            .map(|(a, b)| (String::from(a), String::from(b)))
            .collect::<Vec<(String, String)>>();
        let display_name = match display_name {
            Some(name) => Some(name.to_string()),
            None => None,
        };
        let cfg_options = cfg_options
            .get_cfg_keys()
            .iter()
            .map(|key| {
                (
                    String::from(key.as_str()),
                    cfg_options
                        .get_cfg_values(key)
                        .iter()
                        .map(|val| String::from(val.as_str()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        let potential_cfg_options = potential_cfg_options
            .get_cfg_keys()
            .iter()
            .map(|key| {
                (
                    String::from(key.as_str()),
                    potential_cfg_options
                        .get_cfg_values(key)
                        .iter()
                        .map(|val| String::from(val.as_str()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        self.roots.push(CrateRoot {
            file_id: file_id.0,
            edition: edition.to_string(),
            display_name,
            cfg_options,
            potential_cfg_options,
            env,
            proc_macro: Vec::new(),
        });
        self.roots.len() as u32
    }

    fn contains_dep(&self, from: u32, name: String) -> bool {
        self.deps.iter().any(|dep| dep.from == from && dep.name == name)
    }

    pub fn to_crate_graph<'a>(&self) -> CrateGraph {
        let mut crate_graph = CrateGraph::default();
        self.roots.iter().for_each(|root| {
            let file_id = FileId(root.file_id);
            let edition = root.edition.parse::<Edition>().unwrap_or_else(|err| {
                log::error!("Failed to parse edition {}", err);
                Edition::CURRENT
            });
            let display_name = match &root.display_name {
                Some(name) => Some(CrateDisplayName::from_canonical_name(name.to_string())),
                None => None,
            };
            let cfg_options = parse_cfg_options(&root.cfg_options);
            let potential_cfg_options = parse_cfg_options(&root.potential_cfg_options);
            let mut env = Env::default();
            root.env
                .iter()
                .map(|(key, value)| (key, value))
                .for_each(|(a, b)| env.set(a, b.to_string()));
            crate_graph.add_crate_root(
                file_id,
                edition,
                display_name,
                cfg_options,
                potential_cfg_options,
                env,
                Vec::new(),
            );
        });
        crate_graph
    }

    pub fn cargo_to_json(
        rustc_cfg: Vec<CfgFlag>,
        override_cfg: &CfgOverrides,
        load_proc_macro: &mut dyn FnMut(&AbsPath) -> Vec<ProcMacro>,
        load: &mut dyn FnMut(&AbsPath) -> Option<FileId>,
        cargo: &CargoWorkspace,
        build_scripts: &WorkspaceBuildScripts,
        sysroot: Option<&Sysroot>,
        rustc: &Option<CargoWorkspace>,
    ) -> CrateGraphJson {
        let _p = profile::span("cargo_to_crate_graph");
        let mut crate_graph_json = CrateGraphJson::default();
        let (public_deps, libproc_macro) = match sysroot {
            Some(sysroot) => {
                sysroot_to_crate_graph(&mut crate_graph_json, sysroot, rustc_cfg.clone(), load)
            }
            None => (Vec::new(), None),
        };

        let mut cfg_options = CfgOptions::default();
        cfg_options.extend(rustc_cfg);

        let mut pkg_to_lib_crate = FxHashMap::default();

        // Add test cfg for non-sysroot crates
        cfg_options.insert_atom("test".into());
        cfg_options.insert_atom("debug_assertions".into());

        let mut pkg_crates = FxHashMap::default();
        // Does any crate signal to rust-analyzer that they need the rustc_private crates?
        let mut has_private = false;
        // Next, create crates for each package, target pair
        for pkg in cargo.packages() {
            let mut cfg_options = &cfg_options;
            let mut replaced_cfg_options;
            if let Some(overrides) = override_cfg.get(&cargo[pkg].name) {
                // FIXME: this is sort of a hack to deal with #![cfg(not(test))] vanishing such as seen
                // in ed25519_dalek (#7243), and libcore (#9203) (although you only hit that one while
                // working on rust-lang/rust as that's the only time it appears outside sysroot).
                //
                // A more ideal solution might be to reanalyze crates based on where the cursor is and
                // figure out the set of cfgs that would have to apply to make it active.

                replaced_cfg_options = cfg_options.clone();
                replaced_cfg_options.apply_diff(overrides.clone());
                cfg_options = &replaced_cfg_options;
            };

            has_private |= cargo[pkg].metadata.rustc_private;
            let mut lib_tgt = None;
            for &tgt in cargo[pkg].targets.iter() {
                if let Some(file_id) = load(&cargo[tgt].root) {
                    let crate_id = add_target_crate_root(
                        &mut crate_graph_json,
                        &cargo[pkg],
                        build_scripts.outputs.get(pkg),
                        &cfg_options,
                        load_proc_macro,
                        file_id,
                        &cargo[tgt].name,
                    );
                    if cargo[tgt].kind == TargetKind::Lib {
                        lib_tgt = Some((crate_id, cargo[tgt].name.clone()));
                        pkg_to_lib_crate.insert(pkg, crate_id);
                    }
                    if let Some(proc_macro) = libproc_macro {
                        add_dep(
                            &mut crate_graph_json,
                            crate_id,
                            CrateName::new("proc_macro").unwrap(),
                            proc_macro,
                        );
                    }

                    pkg_crates
                        .entry(pkg)
                        .or_insert_with(Vec::new)
                        .push((crate_id, cargo[tgt].kind));
                }
            }

            // Set deps to the core, std and to the lib target of the current package
            for (from, kind) in pkg_crates.get(&pkg).into_iter().flatten() {
                if let Some((to, name)) = lib_tgt.clone() {
                    if to != *from && *kind != TargetKind::BuildScript {
                        // (build script can not depend on its library target)

                        // For root projects with dashes in their name,
                        // cargo metadata does not do any normalization,
                        // so we do it ourselves currently
                        let name = CrateName::normalize_dashes(&name);
                        add_dep(&mut crate_graph_json, *from, name, to);
                    }
                }
                for (name, krate) in public_deps.iter() {
                    add_dep(&mut crate_graph_json, *from, name.clone(), *krate);
                }
            }
        }

        // Now add a dep edge from all targets of upstream to the lib
        // target of downstream.
        for pkg in cargo.packages() {
            for dep in cargo[pkg].dependencies.iter() {
                let name = CrateName::new(&dep.name).unwrap();
                if let Some(&to) = pkg_to_lib_crate.get(&dep.pkg) {
                    for (from, kind) in pkg_crates.get(&pkg).into_iter().flatten() {
                        if dep.kind == DepKind::Build && *kind != TargetKind::BuildScript {
                            // Only build scripts may depend on build dependencies.
                            continue;
                        }
                        if dep.kind != DepKind::Build && *kind == TargetKind::BuildScript {
                            // Build scripts may only depend on build dependencies.
                            continue;
                        }

                        add_dep(&mut crate_graph_json, *from, name.clone(), to)
                    }
                }
            }
        }

        if has_private {
            // If the user provided a path to rustc sources, we add all the rustc_private crates
            // and create dependencies on them for the crates which opt-in to that
            if let Some(rustc_workspace) = rustc {
                handle_rustc_crates(
                    rustc_workspace,
                    load,
                    &mut crate_graph_json,
                    &cfg_options,
                    load_proc_macro,
                    &mut pkg_to_lib_crate,
                    &public_deps,
                    cargo,
                    &pkg_crates,
                );
            }
        }
        crate_graph_json
    }
}

fn parse_cfg_options(options: &Vec<(String, Vec<String>)>) -> CfgOptions {
    let mut cfg_options = CfgOptions::default();
    options.iter().for_each(|(key, values)| {
        let options = values.iter().map(|value| {
            CfgFlag::from_str(format!("{}={}", key.as_str(), value.as_str()).as_str()).unwrap()
        });
        cfg_options.extend(options);
    });
    cfg_options
}

fn handle_rustc_crates(
    rustc_workspace: &CargoWorkspace,
    load: &mut dyn FnMut(&AbsPath) -> Option<FileId>,
    crate_graph_json: &mut CrateGraphJson,
    cfg_options: &CfgOptions,
    load_proc_macro: &mut dyn FnMut(&AbsPath) -> Vec<ProcMacro>,
    pkg_to_lib_crate: &mut FxHashMap<la_arena::Idx<crate::PackageData>, CrateId>,
    public_deps: &[(CrateName, CrateId)],
    cargo: &CargoWorkspace,
    pkg_crates: &FxHashMap<la_arena::Idx<crate::PackageData>, Vec<(CrateId, TargetKind)>>,
) {
    let mut rustc_pkg_crates = FxHashMap::default();
    // The root package of the rustc-dev component is rustc_driver, so we match that
    let root_pkg =
        rustc_workspace.packages().find(|package| rustc_workspace[*package].name == "rustc_driver");
    // The rustc workspace might be incomplete (such as if rustc-dev is not
    // installed for the current toolchain) and `rustcSource` is set to discover.
    if let Some(root_pkg) = root_pkg {
        // Iterate through every crate in the dependency subtree of rustc_driver using BFS
        let mut queue = VecDeque::new();
        queue.push_back(root_pkg);
        while let Some(pkg) = queue.pop_front() {
            // Don't duplicate packages if they are dependended on a diamond pattern
            // N.B. if this line is ommitted, we try to analyse over 4_800_000 crates
            // which is not ideal
            if rustc_pkg_crates.contains_key(&pkg) {
                continue;
            }
            for dep in &rustc_workspace[pkg].dependencies {
                queue.push_back(dep.pkg);
            }
            for &tgt in rustc_workspace[pkg].targets.iter() {
                if rustc_workspace[tgt].kind != TargetKind::Lib {
                    continue;
                }
                if let Some(file_id) = load(&rustc_workspace[tgt].root) {
                    let crate_id = add_target_crate_root(
                        crate_graph_json,
                        &rustc_workspace[pkg],
                        None,
                        cfg_options,
                        load_proc_macro,
                        file_id,
                        &rustc_workspace[tgt].name,
                    );
                    pkg_to_lib_crate.insert(pkg, crate_id);
                    // Add dependencies on core / std / alloc for this crate
                    for (name, krate) in public_deps.iter() {
                        add_dep(crate_graph_json, crate_id, name.clone(), *krate);
                    }
                    rustc_pkg_crates.entry(pkg).or_insert_with(Vec::new).push(crate_id);
                }
            }
        }
    }
    // Now add a dep edge from all targets of upstream to the lib
    // target of downstream.
    for pkg in rustc_pkg_crates.keys().copied() {
        for dep in rustc_workspace[pkg].dependencies.iter() {
            let name = CrateName::new(&dep.name).unwrap();
            if let Some(&to) = pkg_to_lib_crate.get(&dep.pkg) {
                for &from in rustc_pkg_crates.get(&pkg).into_iter().flatten() {
                    add_dep(crate_graph_json, from, name.clone(), to);
                }
            }
        }
    }
    // Add a dependency on the rustc_private crates for all targets of each package
    // which opts in
    for dep in rustc_workspace.packages() {
        let name = CrateName::normalize_dashes(&rustc_workspace[dep].name);

        if let Some(&to) = pkg_to_lib_crate.get(&dep) {
            for pkg in cargo.packages() {
                let package = &cargo[pkg];
                if !package.metadata.rustc_private {
                    continue;
                }
                for (from, _) in pkg_crates.get(&pkg).into_iter().flatten() {
                    // Avoid creating duplicate dependencies
                    // This avoids the situation where `from` depends on e.g. `arrayvec`, but
                    // `rust_analyzer` thinks that it should use the one from the `rustcSource`
                    // instead of the one from `crates.io`
                    if !crate_graph_json.contains_dep(from.0, name.to_string()) {
                        add_dep(crate_graph_json, *from, name.clone(), to);
                    }
                }
            }
        }
    }
}

fn add_target_crate_root(
    crate_graph: &mut CrateGraphJson,
    pkg: &PackageData,
    build_data: Option<&BuildScriptOutput>,
    cfg_options: &CfgOptions,
    load_proc_macro: &mut dyn FnMut(&AbsPath) -> Vec<ProcMacro>,
    file_id: FileId,
    cargo_name: &str,
) -> CrateId {
    let edition = pkg.edition;
    let cfg_options = {
        let mut opts = cfg_options.clone();
        for feature in pkg.active_features.iter() {
            opts.insert_key_value("feature".into(), feature.into());
        }
        if let Some(cfgs) = build_data.as_ref().map(|it| &it.cfgs) {
            opts.extend(cfgs.iter().cloned());
        }
        opts
    };

    let mut env = Env::default();
    inject_cargo_env(pkg, &mut env);

    if let Some(envs) = build_data.map(|it| &it.envs) {
        for (k, v) in envs {
            env.set(k, v.clone());
        }
    }

    let proc_macro = build_data
        .as_ref()
        .and_then(|it| it.proc_macro_dylib_path.as_ref())
        .map(|it| load_proc_macro(it))
        .unwrap_or_default();

    let display_name = CrateDisplayName::from_canonical_name(cargo_name.to_string());
    let mut potential_cfg_options = cfg_options.clone();
    potential_cfg_options.extend(
        pkg.features
            .iter()
            .map(|feat| CfgFlag::KeyValue { key: "feature".into(), value: feat.0.into() }),
    );

    let crate_id = crate_graph.add_crate_root(
        file_id,
        edition,
        Some(display_name),
        cfg_options,
        potential_cfg_options,
        env,
        proc_macro,
    );

    CrateId(crate_id)
}

fn sysroot_to_crate_graph(
    crate_graph_json: &mut CrateGraphJson,
    sysroot: &Sysroot,
    rustc_cfg: Vec<CfgFlag>,
    load: &mut dyn FnMut(&AbsPath) -> Option<FileId>,
) -> (Vec<(CrateName, CrateId)>, Option<CrateId>) {
    let _p = profile::span("sysroot_to_crate_graph");
    let mut cfg_options = CfgOptions::default();
    cfg_options.extend(rustc_cfg);
    let sysroot_crates: FxHashMap<SysrootCrate, CrateId> = sysroot
        .crates()
        .filter_map(|krate| {
            let file_id = load(&sysroot[krate].root)?;

            let env = Env::default();
            let proc_macro = vec![];
            let display_name = CrateDisplayName::from_canonical_name(sysroot[krate].name.clone());
            let crate_id = crate_graph_json.add_crate_root(
                file_id,
                Edition::CURRENT,
                Some(display_name),
                cfg_options.clone(),
                cfg_options.clone(),
                env,
                proc_macro,
            );
            Some((krate, CrateId(crate_id)))
        })
        .collect();

    for from in sysroot.crates() {
        for &to in sysroot[from].deps.iter() {
            let name = CrateName::new(&sysroot[to].name).unwrap();
            if let (Some(&from), Some(&to)) = (sysroot_crates.get(&from), sysroot_crates.get(&to)) {
                add_dep(crate_graph_json, from, name, to);
            }
        }
    }

    let public_deps = sysroot
        .public_deps()
        .map(|(name, idx)| (CrateName::new(name).unwrap(), sysroot_crates[&idx]))
        .collect::<Vec<_>>();

    let libproc_macro = sysroot.proc_macro().and_then(|it| sysroot_crates.get(&it).copied());
    (public_deps, libproc_macro)
}

fn add_dep(graph: &mut CrateGraphJson, from: CrateId, name: CrateName, to: CrateId) {
    graph.add_dep(Dep { from: from.0, name: name.to_string(), to: to.0 });
}

/// Recreates the compile-time environment variables that Cargo sets.
///
/// Should be synced with
/// <https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-crates>
///
/// FIXME: ask Cargo to provide this data instead of re-deriving.
fn inject_cargo_env(package: &PackageData, env: &mut Env) {
    // FIXME: Missing variables:
    // CARGO_BIN_NAME, CARGO_BIN_EXE_<name>

    let manifest_dir = package.manifest.parent();
    env.set("CARGO_MANIFEST_DIR".into(), manifest_dir.as_os_str().to_string_lossy().into_owned());

    // Not always right, but works for common cases.
    env.set("CARGO".into(), "cargo".into());

    env.set("CARGO_PKG_VERSION".into(), package.version.to_string());
    env.set("CARGO_PKG_VERSION_MAJOR".into(), package.version.major.to_string());
    env.set("CARGO_PKG_VERSION_MINOR".into(), package.version.minor.to_string());
    env.set("CARGO_PKG_VERSION_PATCH".into(), package.version.patch.to_string());
    env.set("CARGO_PKG_VERSION_PRE".into(), package.version.pre.to_string());

    env.set("CARGO_PKG_AUTHORS".into(), String::new());

    env.set("CARGO_PKG_NAME".into(), package.name.clone());
    // FIXME: This isn't really correct (a package can have many crates with different names), but
    // it's better than leaving the variable unset.
    env.set("CARGO_CRATE_NAME".into(), CrateName::normalize_dashes(&package.name).to_string());
    env.set("CARGO_PKG_DESCRIPTION".into(), String::new());
    env.set("CARGO_PKG_HOMEPAGE".into(), String::new());
    env.set("CARGO_PKG_REPOSITORY".into(), String::new());
    env.set("CARGO_PKG_LICENSE".into(), String::new());

    env.set("CARGO_PKG_LICENSE_FILE".into(), String::new());
}
