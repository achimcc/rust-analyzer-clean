//! Fully type-check project and print various stats, like the number of type
//! errors.

use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver};
use proc_macro_api::ProcMacroClient;
use project_model::{
    CargoConfig, ChangeJson, CrateGraphJson, ProjectManifest, ProjectWorkspace,
    WorkspaceBuildScripts,
};
use std::sync::Arc;

use crate::{
    cli::load_cargo::LoadCargoConfig,
    reload::{load_proc_macro, ProjectFolders, SourceRootConfig},
};

use vfs::{loader::Handle, AbsPath, AbsPathBuf};

use crate::cli::flags;

impl flags::Json {
    pub fn run(self) -> anyhow::Result<()> {
        let _p = profile::span("json");
        let cargo_config: CargoConfig = Default::default();
        let load_cargo_config = LoadCargoConfig {
            load_out_dirs_from_check: false,
            with_proc_macro: false,
            prefill_caches: false,
        };
        let root = self.path;
        let root = AbsPathBuf::assert(std::env::current_dir()?.join(root));
        let root = ProjectManifest::discover_single(&root)?;
        let workspace = ProjectWorkspace::load(root, &cargo_config, &|_| {})?;

        let change_json = load_change_json(workspace, &cargo_config, &load_cargo_config, &|_| {})?;

        let json =
            serde_json::to_string(&change_json).expect("serialization of crate_graph must work");
        println!("{}", json);
        Ok(())
    }
}

pub fn load_change_json(
    mut ws: ProjectWorkspace,
    cargo_config: &CargoConfig,
    load_config: &LoadCargoConfig,
    progress: &dyn Fn(String),
) -> Result<ChangeJson> {
    let (sender, receiver) = unbounded();
    let mut vfs = vfs::Vfs::default();
    let mut loader = {
        let loader =
            vfs_notify::NotifyHandle::spawn(Box::new(move |msg| sender.send(msg).unwrap()));
        Box::new(loader)
    };

    let proc_macro_client = if load_config.with_proc_macro {
        let path = AbsPathBuf::assert(std::env::current_exe()?);
        Some(ProcMacroClient::extern_process(path, &["proc-macro"]).unwrap())
    } else {
        None
    };

    ws.set_build_scripts(if load_config.load_out_dirs_from_check {
        ws.run_build_scripts(cargo_config, progress)?
    } else {
        WorkspaceBuildScripts::default()
    });

    let crate_graph_json = ws.to_crate_graph_json(
        &mut |path: &AbsPath| load_proc_macro(proc_macro_client.as_ref(), path),
        &mut |path: &AbsPath| {
            let contents = loader.load_sync(path);
            let path = vfs::VfsPath::from(path.to_path_buf());
            vfs.set_file_contents(path.clone(), contents);
            vfs.file_id(&path)
        },
    )?;

    let project_folders = ProjectFolders::new(&[ws], &[]);
    loader.set_config(vfs::loader::Config {
        load: project_folders.load,
        watch: vec![],
        version: 0,
    });

    log::debug!("crate graph: {:?}", crate_graph_json);

    let change_json =
        load_files(crate_graph_json, project_folders.source_root_config, &mut vfs, &receiver);

    Ok(change_json)
}

fn load_files(
    crate_graph_json: CrateGraphJson,
    source_root_config: SourceRootConfig,
    vfs: &mut vfs::Vfs,
    receiver: &Receiver<vfs::loader::Message>,
) -> ChangeJson {
    let mut change_json = ChangeJson::default();
    // wait until Vfs has loaded all roots
    for task in receiver {
        match task {
            vfs::loader::Message::Progress { n_done, n_total, config_version: _ } => {
                if n_done == n_total {
                    break;
                }
            }
            vfs::loader::Message::Loaded { files } => {
                for (path, contents) in files {
                    vfs.set_file_contents(path.into(), contents);
                }
            }
        }
    }
    let changes = vfs.take_changes();
    for file in changes {
        if file.exists() {
            let contents = vfs.file_contents(file.file_id).to_vec();
            if let Ok(text) = String::from_utf8(contents) {
                change_json.change_file(file.file_id, Some(Arc::new(text)))
            }
        }
    }
    let source_roots = source_root_config.partition(vfs);
    change_json.set_roots(source_roots);

    change_json.set_crate_graph(crate_graph_json);

    change_json
}
