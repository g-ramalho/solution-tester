mod app;
mod benchmark;
mod cli;
mod comparison;
mod discovery;
mod domain;
mod runner;
mod ui;

use anyhow::Result;
use clap::Parser;
use cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    app::run(cli)
}

#[cfg(test)]
mod tests;
