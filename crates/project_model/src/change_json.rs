use std::sync::Arc;

use base_db::{FileId, SourceRoot};

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::CrateGraphJson;

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct ChangeJson {
    crate_graph: CrateGraphJson,
    local_roots: SourceRootJson,
    library_roots: SourceRootJson,
    files: FxHashMap<u32, Option<String>>,
}

impl ChangeJson {
    pub fn change_file(&mut self, file_id: FileId, new_text: Option<Arc<String>>) -> () {
        let new_text = match new_text {
            Some(new_text) => Some(new_text.to_string()),
            None => None,
        };
        let file_id = file_id.0;
        self.files.insert(file_id, new_text);
    }
    pub fn set_roots(&mut self, roots: Vec<SourceRoot>) -> () {
        self.library_roots = SourceRootJson::from_roots(&roots, true);
        self.local_roots = SourceRootJson::from_roots(&roots, false);
    }
    pub fn set_crate_graph(&mut self, crate_graph: CrateGraphJson) -> () {
        self.crate_graph = crate_graph;
    }
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
struct SourceRootJson {
    roots: Vec<(u32, Option<String>)>,
}

impl SourceRootJson {
    pub fn from_roots(roots: &Vec<SourceRoot>, library: bool) -> Self {
        let roots = roots
            .iter()
            .filter(|root| root.is_library == library)
            .flat_map(|val| val.iter().map(move |file_id| (file_id, val.path_for_file(&file_id))))
            .map(|(id, path)| {
                let id = id.0;
                let path = match path {
                    Some(path) => Some(path.to_string()),
                    None => None,
                };
                (id, path)
            })
            .collect::<Vec<(u32, Option<String>)>>();
        SourceRootJson { roots }
    }
    pub fn to_roots(&self) -> Vec<SourceRoot> {
        let _ = self.roots.iter().for_each(|(id, root)| ());
        Vec::new()
    }
}
