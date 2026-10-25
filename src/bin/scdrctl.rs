//! scdrctl: inspect and control the SCDR daemon.

use anyhow::{Context, bail};
use clap::{Args, Parser, Subcommand};
use scdr::config::{Config, Paths};
use scdr::dbus::Daemon1Proxy;
use scdr::explain::{self, Explain, Status};
use scdr::procfs::{self, ProcInfo};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "Inspect and control the SCDR scheduling daemon")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show daemon state and system scheduler facts.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Explain which rule and profile apply to a process, and why.
    Explain {
        /// A pid, or a process name (every process with that name).
        target: String,
        /// Evaluate the config locally instead of asking the daemon.
        #[arg(long)]
        offline: bool,
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        paths: PathArgs,
    },
    /// List running processes and the profile the config gives them (offline).
    List {
        /// Include processes no rule matches.
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        paths: PathArgs,
    },
    /// Validate the configuration files.
    Check {
        #[command(flatten)]
        paths: PathArgs,
    },
    /// Ask the daemon to reload its configuration (root only).
    Reload,
    /// Run the per-user session agent (scdr-session.service): reports the
    /// focused window, GameMode games and PipeWire audio streams to scdrd.
    Agent,
}

#[derive(Args)]
struct PathArgs {
    #[arg(long, default_value = scdr::config::ETC_DIR)]
    etc_dir: PathBuf,
    #[arg(long, default_value = scdr::config::SHARE_DIR)]
    share_dir: PathBuf,
}

impl PathArgs {
    fn load(&self) -> anyhow::Result<Config> {
        let paths = Paths {
            etc: self.etc_dir.clone(),
            share: self.share_dir.clone(),
        };
        let config = Config::load(&paths)?;
        if config.files.is_empty() {
            bail!(
                "no config files in {} or {}",
                paths.share.display(),
                paths.etc.display()
            );
        }
        Ok(config)
    }
}

async fn daemon() -> anyhow::Result<Daemon1Proxy<'static>> {
    let conn = zbus::Connection::system()
        .await
        .context("connecting to the system bus")?;
    Daemon1Proxy::new(&conn)
        .await
        .context("connecting to scdrd")
}

fn not_running(e: zbus::Error) -> anyhow::Error {
    if let zbus::Error::MethodError(name, _, _) = &e {
        if name.as_str() == "org.freedesktop.DBus.Error.AccessDenied" {
            return anyhow::anyhow!("{e}\nThis needs root: try `run0 scdrctl reload` or `systemctl reload scdrd`.");
        }
    }
    anyhow::anyhow!("{e}\nIs scdrd running? Try `systemctl status scdrd`, or use --offline.")
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Status { json } => {
            let raw = daemon().await?.status().await.map_err(not_running)?;
            if json {
                println!("{raw}");
            } else {
                let st: Status = serde_json::from_str(&raw)?;
                print!("{st}");
            }
        }
        Command::Explain {
            target,
            offline,
            json,
            paths,
        } => {
            let pids = resolve(&target)?;
            let config = if offline { Some(paths.load()?) } else { None };
            let proxy = if offline { None } else { Some(daemon().await?) };
            for (i, pid) in pids.iter().enumerate() {
                let ex: Explain = match (&config, &proxy) {
                    (Some(c), _) => explain::offline(c, *pid),
                    (None, Some(p)) => {
                        serde_json::from_str(&p.explain(*pid).await.map_err(not_running)?)?
                    }
                    (None, None) => unreachable!(),
                };
                if json {
                    println!("{}", serde_json::to_string(&ex)?);
                } else {
                    if i > 0 {
                        println!();
                    }
                    print!("{ex}");
                }
            }
        }
        Command::List { all, paths } => list(&paths.load()?, all),
        Command::Check { paths } => {
            let c = paths.load()?;
            println!("ok: {} profiles, {} rules", c.profiles.len(), c.rules.len());
            for f in &c.files {
                println!("  {}", f.display());
            }
        }
        Command::Agent => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info".into()),
                )
                .with_writer(std::io::stderr)
                .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
                .without_time()
                .with_target(false)
                .init();
            scdr::agent::run().await?;
        }
        Command::Reload => {
            let summary = daemon().await?.reload().await.map_err(not_running)?;
            println!("reloaded: {summary}");
        }
    }
    Ok(())
}

/// A pid, or every process whose comm / exe name / argv0 name equals `target`.
fn resolve(target: &str) -> anyhow::Result<Vec<u32>> {
    if let Ok(pid) = target.parse() {
        return Ok(vec![pid]);
    }
    let pids: Vec<u32> = procfs::pids()
        .into_iter()
        .filter(|&pid| ProcInfo::read(pid).is_ok_and(|p| p.names().contains(&target)))
        .collect();
    if pids.is_empty() {
        bail!("no process named {target:?}");
    }
    Ok(pids)
}

fn list(config: &Config, all: bool) {
    println!("{:>7}  {:<18} {:<16} RULE", "PID", "PROFILE", "NAME");
    let mut pids = procfs::pids();
    pids.sort_unstable();
    for pid in pids {
        let Ok(info) = ProcInfo::read(pid) else {
            continue;
        };
        if info.kthread {
            continue;
        }
        let (profile, rule) = if config.exceptions.exception(&info).is_some()
            || config.exceptions.launcher(&info).is_some()
        {
            ("(excepted)".to_owned(), String::new())
        } else if let Some(r) = config.rules.find(&info) {
            let file = r.source.file_name().unwrap_or_default().to_string_lossy();
            (r.profile.clone(), format!("{file} #{}", r.index))
        } else if all {
            ("-".to_owned(), String::new())
        } else {
            continue;
        };
        println!("{pid:>7}  {profile:<18} {:<16} {rule}", info.comm);
    }
}
