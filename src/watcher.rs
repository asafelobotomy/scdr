//! Process events from the kernel's proc connector (netlink, `CN_IDX_PROC`).
//!
//! In-process and event driven: no BPF toolchain, no helper process. Needs
//! CAP_NET_ADMIN; without it the daemon falls back to periodic rescans.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use tokio::sync::mpsc::UnboundedSender;

const NETLINK_CONNECTOR: libc::c_int = 11;
const CN_IDX_PROC: u32 = 1;
const CN_VAL_PROC: u32 = 1;
const PROC_CN_MCAST_LISTEN: u32 = 1;

const PROC_EVENT_NONE: u32 = 0;
const PROC_EVENT_FORK: u32 = 0x1;
const PROC_EVENT_EXEC: u32 = 0x2;
const PROC_EVENT_COMM: u32 = 0x200;
const PROC_EVENT_EXIT: u32 = 0x8000_0000;

const NLMSG_HDRLEN: usize = 16;
const CN_MSG_LEN: usize = 20;
/// what, cpu, timestamp_ns
const PROC_EVENT_HDRLEN: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Fork {
        parent_tgid: u32,
        child_pid: u32,
        child_tgid: u32,
    },
    Exec {
        pid: u32,
        tgid: u32,
    },
    Comm {
        pid: u32,
        tgid: u32,
    },
    Exit {
        pid: u32,
        tgid: u32,
    },
    /// The socket buffer overflowed and events were lost.
    Overrun,
}

/// Subscribes to process events and forwards them from a background thread.
pub fn spawn(tx: UnboundedSender<Event>) -> io::Result<()> {
    let sock = subscribe()?;
    std::thread::Builder::new()
        .name("scdr-cnproc".into())
        .spawn(move || run(sock, tx))?;
    Ok(())
}

fn subscribe() -> io::Result<OwnedFd> {
    // SAFETY: plain socket(2); the fd is owned immediately.
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            NETLINK_CONNECTOR,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd is a fresh, valid descriptor we exclusively own.
    let sock = unsafe { OwnedFd::from_raw_fd(fd) };

    // SAFETY: zeroed sockaddr_nl is a valid value.
    let mut addr: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    addr.nl_family = libc::AF_NETLINK as u16;
    addr.nl_groups = CN_IDX_PROC;
    // SAFETY: addr is a valid sockaddr_nl of the given size.
    let ret = unsafe {
        libc::bind(
            sock.as_raw_fd(),
            &addr as *const _ as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_nl>() as u32,
        )
    };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }

    // Generous receive buffer: builds can exec thousands of processes a second.
    let size: libc::c_int = 4 << 20;
    // SAFETY: valid option pointer and length. Failure only means a smaller buffer.
    unsafe {
        libc::setsockopt(
            sock.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVBUFFORCE,
            &size as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as u32,
        );
    }

    let msg = listen_message();
    // SAFETY: msg is a valid buffer of msg.len() bytes.
    let sent = unsafe {
        libc::send(
            sock.as_raw_fd(),
            msg.as_ptr() as *const libc::c_void,
            msg.len(),
            0,
        )
    };
    if sent < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(sock)
}

/// nlmsghdr + cn_msg + PROC_CN_MCAST_LISTEN.
fn listen_message() -> Vec<u8> {
    let total = NLMSG_HDRLEN + CN_MSG_LEN + 4;
    let mut m = Vec::with_capacity(total);
    m.extend_from_slice(&(total as u32).to_ne_bytes()); // nlmsg_len
    m.extend_from_slice(&(libc::NLMSG_DONE as u16).to_ne_bytes()); // nlmsg_type
    m.extend_from_slice(&0u16.to_ne_bytes()); // nlmsg_flags
    m.extend_from_slice(&0u32.to_ne_bytes()); // nlmsg_seq
    m.extend_from_slice(&std::process::id().to_ne_bytes()); // nlmsg_pid
    m.extend_from_slice(&CN_IDX_PROC.to_ne_bytes()); // cb_id.idx
    m.extend_from_slice(&CN_VAL_PROC.to_ne_bytes()); // cb_id.val
    m.extend_from_slice(&0u32.to_ne_bytes()); // seq
    m.extend_from_slice(&0u32.to_ne_bytes()); // ack
    m.extend_from_slice(&4u16.to_ne_bytes()); // len
    m.extend_from_slice(&0u16.to_ne_bytes()); // flags
    m.extend_from_slice(&PROC_CN_MCAST_LISTEN.to_ne_bytes());
    m
}

fn run(sock: OwnedFd, tx: UnboundedSender<Event>) {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        // SAFETY: buf is valid for buf.len() bytes.
        let n = unsafe {
            libc::recv(
                sock.as_raw_fd(),
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                0,
            )
        };
        if n < 0 {
            let err = io::Error::last_os_error();
            match err.raw_os_error() {
                Some(libc::EINTR) => continue,
                Some(libc::ENOBUFS) => {
                    tracing::warn!("process event buffer overflowed; rescanning");
                    if tx.send(Event::Overrun).is_err() {
                        return;
                    }
                    continue;
                }
                _ => {
                    tracing::error!("process event socket failed: {err}");
                    return;
                }
            }
        }
        for ev in parse(&buf[..n as usize]) {
            if tx.send(ev).is_err() {
                return; // daemon is shutting down
            }
        }
    }
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4).map(|s| u32::from_ne_bytes(s.try_into().unwrap()))
}

/// Parses one datagram, which may hold several netlink messages.
pub fn parse(mut buf: &[u8]) -> Vec<Event> {
    let mut events = Vec::new();
    while buf.len() >= NLMSG_HDRLEN {
        let Some(len) = u32_at(buf, 0).map(|l| l as usize) else {
            break;
        };
        if len < NLMSG_HDRLEN || len > buf.len() {
            break;
        }
        let payload = &buf[NLMSG_HDRLEN..len];
        if let Some(ev) = parse_cn_msg(payload) {
            events.push(ev);
        }
        let aligned = (len + 3) & !3;
        buf = buf.get(aligned..).unwrap_or(&[]);
    }
    events
}

fn parse_cn_msg(p: &[u8]) -> Option<Event> {
    if u32_at(p, 0)? != CN_IDX_PROC || u32_at(p, 4)? != CN_VAL_PROC {
        return None;
    }
    let ev = p.get(CN_MSG_LEN..)?;
    let what = u32_at(ev, 0)?;
    let d = ev.get(PROC_EVENT_HDRLEN..)?;
    Some(match what {
        PROC_EVENT_FORK => Event::Fork {
            parent_tgid: u32_at(d, 4)?,
            child_pid: u32_at(d, 8)?,
            child_tgid: u32_at(d, 12)?,
        },
        PROC_EVENT_EXEC => Event::Exec {
            pid: u32_at(d, 0)?,
            tgid: u32_at(d, 4)?,
        },
        PROC_EVENT_COMM => Event::Comm {
            pid: u32_at(d, 0)?,
            tgid: u32_at(d, 4)?,
        },
        PROC_EVENT_EXIT => Event::Exit {
            pid: u32_at(d, 0)?,
            tgid: u32_at(d, 4)?,
        },
        PROC_EVENT_NONE => {
            // Subscription ack; a non-zero err means the kernel refused us.
            let err = u32_at(d, 0)?;
            if err != 0 {
                tracing::error!("proc connector refused subscription: errno {err}");
            }
            return None;
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(what: u32, data: &[u32]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&CN_IDX_PROC.to_ne_bytes());
        payload.extend_from_slice(&CN_VAL_PROC.to_ne_bytes());
        payload.extend_from_slice(&[0; 8]);
        let ev_len = (PROC_EVENT_HDRLEN + data.len() * 4) as u16;
        payload.extend_from_slice(&ev_len.to_ne_bytes());
        payload.extend_from_slice(&[0; 2]);
        payload.extend_from_slice(&what.to_ne_bytes());
        payload.extend_from_slice(&[0; 12]);
        for d in data {
            payload.extend_from_slice(&d.to_ne_bytes());
        }
        let mut m = Vec::new();
        m.extend_from_slice(&((NLMSG_HDRLEN + payload.len()) as u32).to_ne_bytes());
        m.extend_from_slice(&[0; 12]);
        m.extend_from_slice(&payload);
        while m.len() % 4 != 0 {
            m.push(0);
        }
        m
    }

    #[test]
    fn parses_events_in_one_datagram() {
        let mut dgram = message(PROC_EVENT_FORK, &[10, 10, 11, 11]);
        dgram.extend(message(PROC_EVENT_EXEC, &[11, 11]));
        dgram.extend(message(PROC_EVENT_EXIT, &[11, 11, 0, 17, 10, 10]));
        dgram.extend(message(0x40, &[1, 2])); // PROC_EVENT_SID: ignored
        assert_eq!(
            parse(&dgram),
            vec![
                Event::Fork { parent_tgid: 10, child_pid: 11, child_tgid: 11 },
                Event::Exec { pid: 11, tgid: 11 },
                Event::Exit { pid: 11, tgid: 11 },
            ]
        );
    }

    #[test]
    fn truncated_input_is_ignored() {
        let m = message(PROC_EVENT_EXEC, &[5, 5]);
        assert!(parse(&m[..m.len() - 6]).is_empty());
        assert!(parse(&[1, 2, 3]).is_empty());
    }

    #[test]
    fn listen_message_layout() {
        let m = listen_message();
        assert_eq!(m.len(), 40);
        assert_eq!(u32_at(&m, 0), Some(40));
        assert_eq!(u32_at(&m, 36), Some(PROC_CN_MCAST_LISTEN));
    }
}
