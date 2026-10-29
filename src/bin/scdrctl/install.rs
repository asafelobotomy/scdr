//! `scdrctl schedulers`: get sched_ext schedulers through the system's own
//! package tools, for the widget's Schedulers page. Each tool asks for
//! authentication itself; scdr never installs anything as root.

use anyhow::{Context, bail};
use clap::ValueEnum;
use std::process::Command;

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Action {
    /// Print which installer would be used.
    Which,
    /// Install scx-scheds and scx-tools.
    Install,
    /// Update the system, scx-scheds included (Arch doesn't do partial upgrades).
    Update,
    /// Build scx-scheds-git from the AUR, for schedulers newer than the release.
    Git,
}

const PACKAGES: [&str; 2] = ["scx-scheds", "scx-tools"];
const GIT_PACKAGE: &str = "scx-scheds-git";

#[derive(Clone, Copy)]
enum Installer {
    /// Manjaro's pamac GUI.
    Pamac,
    /// PackageKit, whichever distribution backs it.
    PackageKit,
    /// pacman (or an AUR helper) in a terminal window.
    Terminal,
}

impl Installer {
    fn detect() -> Option<Self> {
        if on_path("pamac-installer") {
            Some(Installer::Pamac)
        } else if on_path("pkcon") {
            Some(Installer::PackageKit)
        } else if on_path("pacman") && terminal().is_some() {
            Some(Installer::Terminal)
        } else {
            None
        }
    }

    fn name(self) -> &'static str {
        match self {
            Installer::Pamac => "pamac",
            Installer::PackageKit => "packagekit",
            Installer::Terminal => "terminal",
        }
    }

    /// The command to run, or why this installer can't do `action`.
    fn command(self, action: Action) -> anyhow::Result<Vec<String>> {
        let v = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        Ok(match (self, action) {
            (_, Action::Which) => unreachable!(),
            (Installer::Pamac, Action::Install) => [v(&["pamac-installer"]), v(&PACKAGES)].concat(),
            (Installer::Pamac, Action::Update) => v(&["pamac-manager", "--updates"]),
            (Installer::Pamac, Action::Git) => v(&["pamac-installer", "--build", GIT_PACKAGE]),
            (Installer::PackageKit, Action::Install) => [v(&["pkcon", "install"]), v(&PACKAGES)].concat(),
            (Installer::PackageKit, Action::Update) => v(&["pkcon", "update"]),
            (Installer::PackageKit, Action::Git) => bail!("PackageKit can't build AUR packages; use an AUR helper"),
            (Installer::Terminal, action) => {
                let root = if on_path("run0") { "run0" } else { "sudo" };
                let line = match action {
                    Action::Install => format!("{root} pacman -S --needed {}", PACKAGES.join(" ")),
                    Action::Update => format!("{root} pacman -Syu"),
                    _ => match ["paru", "yay"].into_iter().find(|h| on_path(h)) {
                        Some(helper) => format!("{helper} -S {GIT_PACKAGE}"),
                        None => bail!("no AUR helper (paru or yay) found to build {GIT_PACKAGE}"),
                    },
                };
                let script = format!("{line}; printf '\\nDone. Press Enter to close.'; read -r _");
                let mut cmd = terminal().context("no terminal emulator found")?;
                cmd.extend(v(&["sh", "-c", &script]));
                cmd
            }
        })
    }
}

pub(crate) fn run(action: Action, dry_run: bool) -> anyhow::Result<()> {
    let Some(installer) = Installer::detect() else {
        bail!("no package installer found (pamac, PackageKit or pacman); install {} yourself", PACKAGES.join(" and "));
    };
    if action == Action::Which {
        println!("{}", installer.name());
        return Ok(());
    }
    let argv = installer.command(action)?;
    if dry_run {
        println!("{}", argv.join(" "));
        return Ok(());
    }
    let status = Command::new(&argv[0])
        .args(&argv[1..])
        .status()
        .with_context(|| format!("starting {}", argv[0]))?;
    if !status.success() {
        bail!("{} exited with {status}", argv[0]);
    }
    Ok(())
}

/// A terminal emulator and the arguments that make it run a command.
fn terminal() -> Option<Vec<String>> {
    const TERMINALS: [(&str, &[&str]); 8] = [
        ("xdg-terminal-exec", &[]),
        ("konsole", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("kgx", &["--"]),
        ("ptyxis", &["--"]),
        ("alacritty", &["-e"]),
        ("foot", &[]),
        ("xterm", &["-e"]),
    ];
    let (bin, args) = TERMINALS.into_iter().find(|(bin, _)| on_path(bin))?;
    Some(std::iter::once(bin).chain(args.iter().copied()).map(String::from).collect())
}

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|dir| dir.join(bin).is_file()))
}
