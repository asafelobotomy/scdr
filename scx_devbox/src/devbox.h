/* SPDX-License-Identifier: GPL-2.0 */
/* Shared between the BPF scheduler and its userspace loader. */
#ifndef __DEVBOX_H
#define __DEVBOX_H

enum devbox_stat {
	DST_DIRECT_IDLE,   /* woken onto an idle CPU straight from select_cpu */
	DST_KTHREAD,       /* per-CPU kthread dispatched locally */
	DST_INTERACTIVE,   /* queued on the interactive (deadline) DSQ */
	DST_BATCH,         /* queued on the batch DSQ */
	DST_HINTED,        /* task carried a scdr latency hint (custom slice) */
	DST_PREEMPT_KICK,  /* interactive wakeup preempted a batch task */
	DST_IDLE_KICK,     /* woke an idle CPU for a queued task */
	DST_STARVE_RESCUE, /* batch served ahead of interactive (starvation guard) */
	DST_HINT_DEMOTED,  /* nice<0 / hinted task demoted to batch as a hog */
	DST_CGROUP_WEIGHTED, /* enqueue of a task whose cgroup factor is not 1.0 */
	DST_PERF_SET,      /* CPU frequency target sent (scx_bpf_cpuperf_set) */
	DST_FOCUS,         /* enqueue of a task in the focused app (scdr hint) */
	DST_BG_DAMPED,     /* enqueue of an unfocused app's task under contention */
	DEVBOX_NR_STATS,
};

/* Per-CPU frequency state (devbox.perf.h); the loader reads perf for stats. */
struct cpu_perf {
	__u64 busy_since;	/* when the running task started; 0 when idle */
	__u64 busy;		/* busy time in the current window */
	__u64 win_start;
	__u32 util;		/* smoothed utilisation, of SCX_CPUPERF_ONE */
	__u32 perf;		/* last target sent; 0 = none yet (kernel default: max) */
};

/* Focus hint slots (devbox.focus.h, hints.h). */
#define MAX_HINTS 4

#define EXIT_MSG_LEN 256

#endif
