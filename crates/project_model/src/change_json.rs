use std::sync::Arc;

use base_db::{FileId, SourceRoot};

use serde::{Deserialize, Serialize};

use crate::CrateGraphJson;

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct ChangeJson {
    crate_graph: CrateGraphJson,
    roots: SourceRootJson,
    files: Vec<(u32, Option<String>)>,
}

impl ChangeJson {
    pub fn change_file(&mut self, file_id: FileId, new_text: Option<Arc<String>>) -> () {
        let new_text = match new_text {
            Some(new_text) => Some(new_text.to_string()),
            None => None,
        };
        let file_id = file_id.0;
        self.files.push((file_id, new_text));
    }
    pub fn set_roots(&mut self, roots: Vec<SourceRoot>) -> () {
        self.roots = SourceRootJson::from_roots(&roots);
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
    pub fn from_roots(roots: &Vec<SourceRoot>) -> Self {
        let roots = roots
            .iter()
            .flat_map(|val| val.iter().map(move |tst| (tst, val.path_for_file(&tst))))
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
}
