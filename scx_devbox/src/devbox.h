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
	DEVBOX_NR_STATS,
};

#define EXIT_MSG_LEN 256

#endif
