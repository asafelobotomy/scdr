//! Reading process identity from `/proc`.

use serde::Serialize;
use std::fs;
use std::io;

const PF_KTHREAD: u64 = 0x0020_0000;

/// The fields of `/proc/PID/stat` we care about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    pub comm: String,
    pub ppid: u32,
    pub flags: u64,
    /// Start time in clock ticks since boot. Together with the pid this
    /// identifies a process uniquely, which guards against pid reuse.
    pub start_time: u64,
}

impl Stat {
    pub fn read(pid: u32) -> io::Result<Self> {
        Self::parse(&fs::read_to_string(format!("/proc/{pid}/stat"))?)
    }

    pub fn parse(s: &str) -> io::Result<Self> {
        let bad = || io::Error::new(io::ErrorKind::InvalidData, "malformed /proc/PID/stat");
        // comm may contain spaces and parentheses, so split on the first '(' and last ')'.
        let open = s.find('(').ok_or_else(bad)?;
        let close = s.rfind(')').ok_or_else(bad)?;
        let comm = s.get(open + 1..close).ok_or_else(bad)?.to_owned();
        // Fields after comm start at field 3 (state), so field N is at index N - 3.
        let rest: Vec<&str> = s[close + 1..].split_ascii_whitespace().collect();
        let field = |n: usize| -> io::Result<u64> {
            rest.get(n - 3).and_then(|v| v.parse().ok()).ok_or_else(bad)
        };
        Ok(Stat {
            comm,
            ppid: field(4)? as u32,
            flags: field(9)?,
            start_time: field(22)?,
        })
    }

    pub fn is_kernel_thread(&self) -> bool {
        self.flags & PF_KTHREAD != 0
    }
}

/// Everything rules can match a process on.
#[derive(Debug, Clone, Serialize)]
pub struct ProcInfo {
    pub pid: u32,
    pub ppid: u32,
    #[serde(skip)]
    pub start_time: u64,
    pub comm: String,
    pub exe: Option<String>,
    pub cmdline: Vec<String>,
    pub cgroup: String,
    pub uid: u32,
    #[serde(skip)]
    pub kthread: bool,
}

impl ProcInfo {
    pub fn read(pid: u32) -> io::Result<Self> {
        let stat = Stat::read(pid)?;
        let kthread = stat.is_kernel_thread();

        let exe = fs::read_link(format!("/proc/{pid}/exe")).ok().map(|p| {
            let s = p.to_string_lossy().into_owned();
            match s.strip_suffix(" (deleted)") {
                Some(stripped) => stripped.to_owned(),
                None => s,
            }
        });

        let cmdline = fs::read(format!("/proc/{pid}/cmdline"))
            .map(|raw| {
                raw.split(|&b| b == 0)
                    .filter(|a| !a.is_empty())
                    .map(|a| String::from_utf8_lossy(a).into_owned())
                    .collect()
            })
            .unwrap_or_default();

        let cgroup = fs::read_to_string(format!("/proc/{pid}/cgroup"))
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("0::").map(str::to_owned))
            })
            .unwrap_or_default();

        let uid = fs::read_to_string(format!("/proc/{pid}/status"))
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("Uid:"))
                    .and_then(|v| v.split_ascii_whitespace().next()?.parse().ok())
            })
            .unwrap_or(u32::MAX);

        Ok(ProcInfo {
            pid,
            ppid: stat.ppid,
            start_time: stat.start_time,
            comm: stat.comm,
            exe,
            cmdline,
            cgroup,
            uid,
            kthread,
        })
    }

    /// Names a `name =` rule is matched against: comm, the executable's file
    /// name, and argv[0]'s file name (which also covers Wine's `C:\…\game.exe`).
    pub fn names(&self) -> Vec<&str> {
        let mut names = vec![self.comm.as_str()];
        for path in [self.exe.as_deref(), self.cmdline.first().map(String::as_str)]
            .into_iter()
            .flatten()
        {
            let name = basename(path);
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    pub fn cmdline_string(&self) -> String {
        self.cmdline.join(" ")
    }
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Reads just `/proc/PID/comm`; cheap enough for walking ancestors.
pub fn comm(pid: u32) -> Option<String> {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim_end_matches('\n').to_owned())
}

fn numeric_entries(dir: &str) -> Vec<u32> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
        .collect()
}

pub fn pids() -> Vec<u32> {
    numeric_entries("/proc")
}

pub fn tids(pid: u32) -> Vec<u32> {
    numeric_entries(&format!("/proc/{pid}/task"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stat_with_awkward_comm() {
        let line = "1234 (we(ird) n) S 1 1234 1234 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 3 0 98765 1000 200 18446744073709551615";
        let st = Stat::parse(line).unwrap();
        assert_eq!(st.comm, "we(ird) n");
        assert_eq!(st.ppid, 1);
        assert_eq!(st.flags, 4194560);
        assert_eq!(st.start_time, 98765);
        assert!(!st.is_kernel_thread());
    }

    #[test]
    fn read_self() {
        let pid = std::process::id();
        let info = ProcInfo::read(pid).unwrap();
        assert_eq!(info.pid, pid);
        assert!(!info.kthread);
        assert!(info.exe.is_some());
        assert!(tids(pid).contains(&pid));
    }

    #[test]
    fn names_cover_exe_and_wine_argv0() {
        let info = ProcInfo {
            pid: 1,
            ppid: 0,
            start_time: 0,
            comm: "wine64-preloade".into(),
            exe: Some("/usr/bin/wine64-preloader".into()),
            cmdline: vec![r"C:\Games\Foo\foo.exe".into()],
            cgroup: String::new(),
            uid: 1000,
            kthread: false,
        };
        assert_eq!(info.names(), vec!["wine64-preloade", "wine64-preloader", "foo.exe"]);
    }
}
