//! Fully type-check project and print various stats, like the number of type
//! errors.

use anyhow::Result;
use crossbeam_channel::unbounded;
use proc_macro_api::ProcMacroClient;
use project_model::{
    CargoConfig, CrateGraphJson, ProjectManifest, ProjectWorkspace, WorkspaceBuildScripts,
};

use crate::{
    cli::load_cargo::LoadCargoConfig,
    reload::{load_proc_macro, ProjectFolders},
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

        let crate_graph_json =
            load_workspace(workspace, &cargo_config, &load_cargo_config, &|_| {})?;

        // let (_, change2) = get_crate_data(root, &|_| {})?;

        let json = serde_json::to_string(&crate_graph_json)
            .expect("serialization of crate_graph must work");
        println!("{}", json);

        let crate_graph = crate_graph_json.to_crate_graph();

        println!("Conversion successful: {:?}", crate_graph);

        // println!("change_json:\n{}", change_json);

        // deserialize from json string
        /*
        let deserialized_crate_graph: CrateGraph =
            serde_json::from_str(&json).expect("deserialization must work");
        assert_eq!(
            crate_graph, deserialized_crate_graph,
            "Deserialized `CrateGraph` is not equal!"
        );
        */

        // Missing: Create a new `Change` object.
        //
        // `serde::Serialize` and `serde::Deserialize` are already supported by `Change`.
        // So this should work out of the box after the object has been created:
        //
        // ```
        // let json = serde_json::to_string(&change).expect("`Change` serialization must work");
        // println!("change json:\n{}", json);
        // let deserialized_change: Change = serde_json::from_str(&json).expect("`Change` deserialization must work");
        // ```

        Ok(())
    }
}

pub fn load_workspace(
    mut ws: ProjectWorkspace,
    cargo_config: &CargoConfig,
    load_config: &LoadCargoConfig,
    progress: &dyn Fn(String),
) -> Result<CrateGraphJson> {
    let (sender, _receiver) = unbounded();
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

    let crate_graph = ws.to_crate_graph_json(
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

    log::debug!("crate graph: {:?}", crate_graph);

    Ok(crate_graph)
}
