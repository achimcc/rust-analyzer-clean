//! Read project from Cargo.toml convert to rust-project.json

use crate::cli::flags;

impl flags::Json {
    pub fn run(self) -> anyhow::Result<()> {
        let _p = profile::span("json");
        println!("Json!");
        Ok(())
    }
}
