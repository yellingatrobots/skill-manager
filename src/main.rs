mod archive;
mod cli;
mod commands;
mod deploy;
mod error;
mod fetch;
mod frontmatter;
mod fsx;
mod generate;
mod git;
mod http;
mod lock;
mod manifest;
mod manifest_edit;
mod paths;
mod skill;
mod store;
mod tree;
mod wellknown;

use clap::Parser;

fn main() -> std::process::ExitCode {
    let code = match cli::run(cli::Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("skill-manager: {error}");
            cli::EXIT_ABORTED
        }
    };
    std::process::ExitCode::from(code as u8)
}
