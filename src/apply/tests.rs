use super::*;
use crate::sched::{IoPrio, Policy};

fn base() -> Snapshot {
    Snapshot {
        task: TaskState {
            cpu: CpuState {
                policy: sched::SCHED_OTHER,
                priority: 0,
                nice: 0,
                slice_ns: 0,
                util_min: 0,
                util_max: 1024,
            },
            ioprio: 0,
        },
        oom: Some(0),
    }
}

#[test]
fn profile_overlay() {
    let p = Profile {
        nice: Some(5),
        sched: Some(Policy::Batch),
        slice_us: Some(1000),
        io: Some(IoPrio::Idle),
        ..Default::default()
    };
    let t = base().with_profile(&p);
    assert_eq!(t.task.cpu.policy, sched::SCHED_BATCH);
    assert_eq!(t.task.cpu.nice, 5);
    assert_eq!(t.task.cpu.slice_ns, 1_000_000);
    assert_eq!(t.task.ioprio, IoPrio::Idle.to_kernel());
    assert_eq!(t.oom, Some(0));
    assert_eq!(t.to_string(), "policy=batch nice=5 slice=1000us io=idle oom_score_adj=0");
}

#[test]
fn realtime_profile_drops_slice() {
    let p = Profile {
        sched: Some(Policy::Fifo(10)),
        slice_us: Some(500),
        ..Default::default()
    };
    let t = base().with_profile(&p);
    assert_eq!(t.task.cpu.policy, sched::SCHED_FIFO);
    assert_eq!(t.task.cpu.priority, 10);
    assert_eq!(t.task.cpu.slice_ns, 0);
}

/// Runs `f` with the tids of two helper threads: one untouched, one that
/// "someone else" reniced to 19.
fn with_helper_threads(f: impl FnOnce(u32, u32)) {
    use std::sync::{Arc, Barrier, mpsc};
    let done = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();
    let handles: Vec<_> = (0..2)
        .map(|i| {
            let (done, tx) = (done.clone(), tx.clone());
            std::thread::spawn(move || {
                let tid = unsafe { libc::gettid() } as u32;
                if i == 1 {
                    let mut a = sched::get_attr(tid).unwrap();
                    a.nice = 19;
                    sched::set_attr(tid, &a).unwrap();
                }
                tx.send((i, tid)).unwrap();
                done.wait();
            })
        })
        .collect();
    let mut tids = [0; 2];
    for _ in 0..2 {
        let (i, tid) = rx.recv().unwrap();
        tids[i] = tid;
    }
    f(tids[0], tids[1]);
    done.wait();
    handles.into_iter().for_each(|h| h.join().unwrap());
}

#[test]
fn changes_owned_threads_and_leaves_foreign_ones() {
    with_helper_threads(|plain, reniced| {
        let original = Snapshot::original(plain).unwrap();
        if original.task.cpu.policy != sched::SCHED_OTHER || original.task.cpu.nice > 15 {
            return; // unusual test environment
        }
        let target = original.with_profile(&Profile {
            nice: Some(original.task.cpu.nice + 2),
            slice_us: Some(2000),
            io: Some(IoPrio::BestEffort(7)),
            ..Default::default()
        });
        let reniced_io = TaskState::read(reniced).unwrap().ioprio;
        let tids = [plain, reniced];

        let dry = apply_threads(&tids, &target, &[original], true);
        assert_eq!((dry.changed, dry.foreign), (1, 1), "{dry:?}");
        assert_eq!(TaskState::read(plain).unwrap().cpu.nice, original.task.cpu.nice);

        let real = apply_threads(&tids, &target, &[original], false);
        assert_eq!((real.changed, real.foreign), (1, 1), "{real:?}");
        assert!(real.errors.is_empty(), "{real:?}");
        assert_eq!(TaskState::read(plain).unwrap().cpu, target.task.cpu);
        assert_eq!(TaskState::read(plain).unwrap().ioprio, target.task.ioprio);
        // The foreign thread is untouched as a whole, io included.
        assert_eq!(TaskState::read(reniced).unwrap().cpu.nice, 19);
        assert_eq!(TaskState::read(reniced).unwrap().ioprio, reniced_io);

        // A second pass is a no-op.
        let again = apply_threads(&tids, &target, &[original], false);
        assert_eq!((again.changed, again.unchanged), (0, 1), "{again:?}");

        // Restoring clears the custom slice we set (nice can't go back
        // down unprivileged, so restore to a nice the thread can reach).
        let restore = Snapshot {
            task: TaskState {
                cpu: CpuState { nice: target.task.cpu.nice, ..original.task.cpu },
                ..original.task
            },
            ..original
        };
        let back = apply_threads(&[plain], &restore, &[target], false);
        assert_eq!(back.changed, 1, "{back:?}");
        assert_ne!(TaskState::read(plain).unwrap().cpu.slice_ns, 2_000_000);
    });
}

#[test]
fn uclamp_is_set_and_cleared() {
    with_helper_threads(|plain, _| {
        let original = Snapshot::original(plain).unwrap();
        if original.task.cpu.policy != sched::SCHED_OTHER {
            return;
        }
        let target = original.with_profile(&Profile {
            uclamp_max: Some(512),
            ..Default::default()
        });
        assert!(target.to_string().contains("uclamp=0..512"), "{target}");

        let out = apply_threads(&[plain], &target, &[original], false);
        assert_eq!(out.changed, 1, "{out:?}");
        let cur = TaskState::read(plain).unwrap().cpu;
        assert_eq!((cur.util_min, cur.util_max), (0, 512));

        // Clearing resets to the kernel default rather than pinning 0..1024.
        let back = apply_threads(&[plain], &original, &[target], false);
        assert_eq!(back.changed, 1, "{back:?}");
        let cur = TaskState::read(plain).unwrap().cpu;
        assert_eq!((cur.util_min, cur.util_max), (0, 1024));
    });
}
