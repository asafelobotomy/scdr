//! Thin wrappers over the kernel's per-task scheduling interfaces.
//!
//! Everything here operates on a single task (thread) id. Callers walk
//! `/proc/PID/task` themselves so that every thread gets the same treatment.

use serde::Serialize;
use std::fmt;
use std::io;
use std::str::FromStr;

pub const SCHED_OTHER: u32 = 0;
pub const SCHED_FIFO: u32 = 1;
pub const SCHED_RR: u32 = 2;
pub const SCHED_BATCH: u32 = 3;
pub const SCHED_IDLE: u32 = 5;
pub const SCHED_DEADLINE: u32 = 6;
pub const SCHED_EXT: u32 = 7;

pub const SCHED_FLAG_RESET_ON_FORK: u64 = 0x01;

/// EEVDF clamps custom slices to this range (`kernel/sched/fair.c:__setparam_fair`).
pub const SLICE_MIN_US: u32 = 100;
pub const SLICE_MAX_US: u32 = 100_000;

/// `struct sched_attr` (SCHED_ATTR_SIZE_VER1, 56 bytes).
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SchedAttr {
    pub size: u32,
    pub policy: u32,
    pub flags: u64,
    pub nice: i32,
    pub priority: u32,
    /// For fair-class tasks this is the EEVDF slice in ns; 0 means "use base_slice_ns".
    pub runtime: u64,
    pub deadline: u64,
    pub period: u64,
    pub util_min: u32,
    pub util_max: u32,
}

const ATTR_SIZE: u32 = std::mem::size_of::<SchedAttr>() as u32;

pub fn get_attr(tid: u32) -> io::Result<SchedAttr> {
    let mut attr = SchedAttr::default();
    // SAFETY: attr is a valid, writable sched_attr of ATTR_SIZE bytes.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_sched_getattr,
            tid as libc::pid_t,
            &mut attr as *mut SchedAttr,
            ATTR_SIZE,
            0u32,
        )
    };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(attr)
}

pub fn set_attr(tid: u32, attr: &SchedAttr) -> io::Result<()> {
    let mut attr = *attr;
    attr.size = ATTR_SIZE;
    // SAFETY: attr is a valid sched_attr that outlives the call.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_sched_setattr,
            tid as libc::pid_t,
            &attr as *const SchedAttr,
            0u32,
        )
    };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

const IOPRIO_WHO_PROCESS: libc::c_int = 1;
const IOPRIO_CLASS_SHIFT: u32 = 13;

/// Reads the I/O priority of one task. 0 means "none" (derived from nice).
pub fn get_ioprio(tid: u32) -> io::Result<u32> {
    // SAFETY: plain syscall with integer arguments.
    let ret = unsafe { libc::syscall(libc::SYS_ioprio_get, IOPRIO_WHO_PROCESS, tid as libc::c_int) };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(ret as u32)
}

pub fn set_ioprio(tid: u32, ioprio: u32) -> io::Result<()> {
    // SAFETY: plain syscall with integer arguments.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_ioprio_set,
            IOPRIO_WHO_PROCESS,
            tid as libc::c_int,
            ioprio as libc::c_int,
        )
    };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn get_oom_score_adj(pid: u32) -> io::Result<i32> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/oom_score_adj"))?;
    s.trim()
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad oom_score_adj"))
}

pub fn set_oom_score_adj(pid: u32, value: i32) -> io::Result<()> {
    std::fs::write(format!("/proc/{pid}/oom_score_adj"), value.to_string())
}

/// A scheduling policy as written in config: `other`, `batch`, `idle`, `fifo:N`, `rr:N`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    Other,
    Batch,
    Idle,
    Fifo(u8),
    Rr(u8),
}

impl Policy {
    pub fn kernel_policy(self) -> u32 {
        match self {
            Policy::Other => SCHED_OTHER,
            Policy::Batch => SCHED_BATCH,
            Policy::Idle => SCHED_IDLE,
            Policy::Fifo(_) => SCHED_FIFO,
            Policy::Rr(_) => SCHED_RR,
        }
    }

    pub fn rt_priority(self) -> u32 {
        match self {
            Policy::Fifo(p) | Policy::Rr(p) => p as u32,
            _ => 0,
        }
    }

    pub fn is_realtime(self) -> bool {
        matches!(self, Policy::Fifo(_) | Policy::Rr(_))
    }

    pub fn from_kernel(policy: u32, priority: u32) -> Option<Self> {
        Some(match policy {
            SCHED_OTHER => Policy::Other,
            SCHED_BATCH => Policy::Batch,
            SCHED_IDLE => Policy::Idle,
            SCHED_FIFO => Policy::Fifo(priority as u8),
            SCHED_RR => Policy::Rr(priority as u8),
            _ => return None,
        })
    }
}

impl FromStr for Policy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let rt = |p: &str| -> Result<u8, String> {
            match p.parse::<u8>() {
                Ok(n @ 1..=99) => Ok(n),
                _ => Err(format!("realtime priority must be 1-99, got {p:?}")),
            }
        };
        match s.split_once(':') {
            None => match s {
                "other" | "normal" => Ok(Policy::Other),
                "batch" => Ok(Policy::Batch),
                "idle" => Ok(Policy::Idle),
                "fifo" | "rr" => Err(format!("{s} needs a priority, e.g. \"{s}:10\"")),
                _ => Err(format!("unknown sched policy {s:?} (other, batch, idle, fifo:N, rr:N)")),
            },
            Some(("fifo", p)) => Ok(Policy::Fifo(rt(p)?)),
            Some(("rr", p)) => Ok(Policy::Rr(rt(p)?)),
            Some(_) => Err(format!("unknown sched policy {s:?} (other, batch, idle, fifo:N, rr:N)")),
        }
    }
}

impl fmt::Display for Policy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Policy::Other => f.write_str("other"),
            Policy::Batch => f.write_str("batch"),
            Policy::Idle => f.write_str("idle"),
            Policy::Fifo(p) => write!(f, "fifo:{p}"),
            Policy::Rr(p) => write!(f, "rr:{p}"),
        }
    }
}

impl Serialize for Policy {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

/// An I/O priority as written in config: `idle`, `be:N`, `rt:N` (N = 0..7, 0 highest).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoPrio {
    Realtime(u8),
    BestEffort(u8),
    Idle,
}

impl IoPrio {
    pub fn to_kernel(self) -> u32 {
        match self {
            IoPrio::Realtime(l) => (1 << IOPRIO_CLASS_SHIFT) | l as u32,
            IoPrio::BestEffort(l) => (2 << IOPRIO_CLASS_SHIFT) | l as u32,
            IoPrio::Idle => 3 << IOPRIO_CLASS_SHIFT,
        }
    }

    /// `None` for class 0 ("none": the kernel derives it from nice).
    pub fn from_kernel(v: u32) -> Option<Self> {
        let level = (v & 0x7) as u8;
        match v >> IOPRIO_CLASS_SHIFT {
            1 => Some(IoPrio::Realtime(level)),
            2 => Some(IoPrio::BestEffort(level)),
            3 => Some(IoPrio::Idle),
            _ => None,
        }
    }
}

impl FromStr for IoPrio {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let level = |p: &str| -> Result<u8, String> {
            match p.parse::<u8>() {
                Ok(n @ 0..=7) => Ok(n),
                _ => Err(format!("io level must be 0-7, got {p:?}")),
            }
        };
        match s.split_once(':') {
            None if s == "idle" => Ok(IoPrio::Idle),
            Some(("be", l)) => Ok(IoPrio::BestEffort(level(l)?)),
            Some(("rt", l)) => Ok(IoPrio::Realtime(level(l)?)),
            _ => Err(format!("unknown io priority {s:?} (idle, be:0-7, rt:0-7)")),
        }
    }
}

impl fmt::Display for IoPrio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IoPrio::Realtime(l) => write!(f, "rt:{l}"),
            IoPrio::BestEffort(l) => write!(f, "be:{l}"),
            IoPrio::Idle => f.write_str("idle"),
        }
    }
}

impl Serialize for IoPrio {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attr_layout_matches_kernel_ver1() {
        assert_eq!(ATTR_SIZE, 56);
    }

    #[test]
    fn parse_policy() {
        assert_eq!("batch".parse(), Ok(Policy::Batch));
        assert_eq!("normal".parse(), Ok(Policy::Other));
        assert_eq!("fifo:10".parse(), Ok(Policy::Fifo(10)));
        assert!("fifo".parse::<Policy>().is_err());
        assert!("rr:0".parse::<Policy>().is_err());
        assert!("deadline".parse::<Policy>().is_err());
    }

    #[test]
    fn parse_ioprio_roundtrip() {
        for s in ["idle", "be:0", "be:7", "rt:3"] {
            let p: IoPrio = s.parse().unwrap();
            assert_eq!(p.to_string(), s);
            assert_eq!(IoPrio::from_kernel(p.to_kernel()), Some(p));
        }
        assert!("be:8".parse::<IoPrio>().is_err());
        assert_eq!(IoPrio::from_kernel(0), None);
    }

    #[test]
    fn read_own_attr() {
        let tid = unsafe { libc::gettid() } as u32;
        let attr = get_attr(tid).unwrap();
        assert!(Policy::from_kernel(attr.policy, attr.priority).is_some() || attr.policy == SCHED_EXT);
    }

    #[test]
    fn set_slice_and_nice_on_own_thread() {
        // Unprivileged tasks may raise their own nice and set a custom slice.
        std::thread::spawn(|| {
            let tid = unsafe { libc::gettid() } as u32;
            let mut attr = get_attr(tid).unwrap();
            if attr.policy != SCHED_OTHER {
                return;
            }
            attr.nice = attr.nice.max(5);
            attr.runtime = 1_500_000;
            set_attr(tid, &attr).unwrap();
            let after = get_attr(tid).unwrap();
            assert_eq!(after.nice, attr.nice);
            assert_eq!(after.runtime, 1_500_000);
        })
        .join()
        .unwrap();
    }
}
