//! `rust-project.json` file format.
//!
//! This format is spiritually a serialization of [`base_db::CrateGraph`]. The
//! idea here is that people who do not use Cargo, can instead teach their build
//! system to generate `rust-project.json` which can be ingested by
//! rust-analyzer.

use std::{path::PathBuf, str::FromStr};

use base_db::{CrateDisplayName, CrateId, CrateName, Dependency, Edition};
use paths::{AbsPath, AbsPathBuf};
use rustc_hash::FxHashMap;
use serde::{de, Deserialize};

use crate::cfg_flag::CfgFlag;

/// Roots and crates that compose this Rust project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectJson {
    pub(crate) sysroot_src: Option<AbsPathBuf>,
    project_root: AbsPathBuf,
    crates: Vec<Crate>,
}

/// A crate points to the root module of a crate and lists the dependencies of the crate. This is
/// useful in creating the crate graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Crate {
    pub(crate) display_name: Option<CrateDisplayName>,
    pub(crate) root_module: AbsPathBuf,
    pub(crate) edition: Edition,
    pub(crate) deps: Vec<Dependency>,
    pub(crate) cfg: Vec<CfgFlag>,
    pub(crate) target: Option<String>,
    pub(crate) env: FxHashMap<String, String>,
    pub(crate) proc_macro_dylib_path: Option<AbsPathBuf>,
    pub(crate) is_workspace_member: bool,
    pub(crate) include: Vec<AbsPathBuf>,
    pub(crate) exclude: Vec<AbsPathBuf>,
    pub(crate) is_proc_macro: bool,
}

impl ProjectJson {
    /// Create a new ProjectJson instance.
    ///
    /// # Arguments
    ///
    /// * `base` - The path to the workspace root (i.e. the folder containing `rust-project.json`)
    /// * `data` - The parsed contents of `rust-project.json`, or project json that's passed via
    ///            configuration.
    pub fn new(base: &AbsPath, data: ProjectJsonData) -> ProjectJson {
        ProjectJson {
            sysroot_src: data.sysroot_src.map(|it| base.join(it)),
            project_root: base.to_path_buf(),
            crates: data
                .crates
                .into_iter()
                .map(|crate_data| {
                    let is_workspace_member = crate_data.is_workspace_member.unwrap_or_else(|| {
                        crate_data.root_module.is_relative()
                            && !crate_data.root_module.starts_with("..")
                            || crate_data.root_module.starts_with(base)
                    });
                    let root_module = base.join(crate_data.root_module).normalize();
                    let (include, exclude) = match crate_data.source {
                        Some(src) => {
                            let absolutize = |dirs: Vec<PathBuf>| {
                                dirs.into_iter()
                                    .map(|it| base.join(it).normalize())
                                    .collect::<Vec<_>>()
                            };
                            (absolutize(src.include_dirs), absolutize(src.exclude_dirs))
                        }
                        None => (vec![root_module.parent().unwrap().to_path_buf()], Vec::new()),
                    };

                    Crate {
                        display_name: crate_data
                            .display_name
                            .map(CrateDisplayName::from_canonical_name),
                        root_module,
                        edition: crate_data.edition.into(),
                        deps: crate_data
                            .deps
                            .into_iter()
                            .map(|dep_data| Dependency {
                                crate_id: CrateId(dep_data.krate as u32),
                                name: dep_data.name,
                            })
                            .collect::<Vec<_>>(),
                        cfg: crate_data.cfg,
                        target: crate_data.target,
                        env: crate_data.env,
                        proc_macro_dylib_path: crate_data
                            .proc_macro_dylib_path
                            .map(|it| base.join(it)),
                        is_workspace_member,
                        include,
                        exclude,
                        is_proc_macro: crate_data.is_proc_macro,
                    }
                })
                .collect::<Vec<_>>(),
        }
    }

    /// Returns the number of crates in the project.
    pub fn n_crates(&self) -> usize {
        self.crates.len()
    }
    /// Returns an iterator over the crates in the project.
    pub fn crates(&self) -> impl Iterator<Item = (CrateId, &Crate)> + '_ {
        self.crates.iter().enumerate().map(|(idx, krate)| (CrateId(idx as u32), krate))
    }
    /// Returns the path to the project's root folder.
    pub fn path(&self) -> &AbsPath {
        &self.project_root
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct ProjectJsonData {
    pub(crate) sysroot_src: Option<PathBuf>,
    pub(crate) crates: Vec<CrateData>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CrateData {
    pub(crate) display_name: Option<String>,
    pub(crate) root_module: PathBuf,
    pub(crate) edition: EditionData,
    pub(crate) deps: Vec<DepData>,
    #[serde(default)]
    pub(crate) cfg: Vec<CfgFlag>,
    pub(crate) target: Option<String>,
    #[serde(default)]
    pub(crate) env: FxHashMap<String, String>,
    pub(crate) proc_macro_dylib_path: Option<PathBuf>,
    pub(crate) is_workspace_member: Option<bool>,
    pub(crate) source: Option<CrateSource>,
    #[serde(default)]
    pub(crate) is_proc_macro: bool,
}

#[derive(Debug)]
pub struct ParseEditionError {
    invalid_input: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename = "edition")]
pub enum EditionData {
    #[serde(rename = "2015")]
    Edition2015,
    #[serde(rename = "2018")]
    Edition2018,
    #[serde(rename = "2021")]
    Edition2021,
}

impl From<EditionData> for Edition {
    fn from(data: EditionData) -> Self {
        match data {
            EditionData::Edition2015 => Edition::Edition2015,
            EditionData::Edition2018 => Edition::Edition2018,
            EditionData::Edition2021 => Edition::Edition2021,
        }
    }
}

impl From<Edition> for EditionData {
    fn from(data: Edition) -> Self {
        match data {
            Edition::Edition2015 => EditionData::Edition2015,
            Edition::Edition2018 => EditionData::Edition2018,
            Edition::Edition2021 => EditionData::Edition2021,
        }
    }
}

impl FromStr for EditionData {
    type Err = ParseEditionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let res = match s {
            "2015" => EditionData::Edition2015,
            "2018" => EditionData::Edition2018,
            "2021" => EditionData::Edition2021,
            _ => return Err(ParseEditionError { invalid_input: s.to_string() }),
        };
        Ok(res)
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct DepData {
    /// Identifies a crate by position in the crates array.
    #[serde(rename = "crate")]
    pub(crate) krate: usize,
    #[serde(deserialize_with = "deserialize_crate_name")]
    pub(crate) name: CrateName,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CrateSource {
    include_dirs: Vec<PathBuf>,
    exclude_dirs: Vec<PathBuf>,
}

fn deserialize_crate_name<'de, D>(de: D) -> Result<CrateName, D::Error>
where
    D: de::Deserializer<'de>,
{
    let name = String::deserialize(de)?;
    CrateName::new(&name).map_err(|err| de::Error::custom(format!("invalid crate name: {:?}", err)))
}
