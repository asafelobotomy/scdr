/* SPDX-License-Identifier: GPL-2.0 */
/* Tunables, state, maps and helpers for devbox.bpf.c. */
#ifndef __DEVBOX_BPF_H
#define __DEVBOX_BPF_H

#define BPF_STRUCT_OPS(name, args...) \
	SEC("struct_ops/" #name) BPF_PROG(name, ##args)
#define BPF_STRUCT_OPS_SLEEPABLE(name, args...) \
	SEC("struct_ops.s/" #name) BPF_PROG(name, ##args)

/* DSQ ids double as indexes into vtime_now[]. */
#define INTERACTIVE_DSQ	0
#define BATCH_DSQ	1
#define MAX_CPUS	512	/* power of two, used as an index mask */
#define CPU_IDX(cpu)	((u32)(cpu) & (MAX_CPUS - 1))

#define PF_KTHREAD	0x00200000
#define SCHED_BATCH	3
#define SCHED_IDLE	5
#define NICE_0_PRIO	120

/* Tunables, set by the loader before load. */
const volatile u64 slice_ns = 3000000ULL;	   /* interactive slice */
const volatile u64 slice_min_ns = 500000ULL;	   /* floor under load */
const volatile u64 batch_slice_ns = 12000000ULL;   /* long, cache-friendly */
const volatile u64 batch_thresh_ns = 8000000ULL;   /* avg burst => batch */
const volatile u32 hint_thresh_mult = 4;	   /* x threshold if hinted */
const volatile u64 batch_max_wait_ns = 60000000ULL; /* per-CPU starvation guard */
const volatile u64 lag_ns = 20000000ULL;	   /* max sleep credit */
const volatile bool preempt_batch = true;
const volatile u64 batch_min_run_ns = 1000000ULL; /* before preemptible */
const volatile u32 nr_cpus = 1;

/*
 * Global state. Racy plain accesses are fine: these are heuristics.
 * Each class has its own vtime clock, so a nice-15 compiler racing ahead in
 * the batch queue doesn't drag the interactive clock (and every interactive
 * task's sleep credit) along with it.
 */
u64 vtime_now[2];
u64 batch_nonempty_since;	/* when the batch DSQ last became non-empty */
u64 cpu_batch_since[MAX_CPUS];	/* batch task's start on this CPU; 0 = none */
u64 cpu_batch_served[MAX_CPUS];	/* last time this CPU ran a batch task */

/* Exit info for the loader. */
s32 exit_kind;
s64 exit_code;
char exit_reason[EXIT_MSG_LEN];
char exit_msg[EXIT_MSG_LEN];

struct task_ctx {
	u64 vruntime;	/* weighted runtime on its class's clock */
	u64 avg_burst;	/* EWMA of runtime between sleeps */
	u64 burst;	/* runtime since the task last slept */
	u64 run_at;	/* when it last started running */
	u32 cgw;	/* cached cgroup factor, CGW_ONE = 1.0 (see devbox.cgroup.h) */
	u32 cgw_gen;	/* cgw_gen value cgw was computed under */
	bool batch;
};

struct {
	__uint(type, BPF_MAP_TYPE_TASK_STORAGE);
	__uint(map_flags, BPF_F_NO_PREALLOC);
	__type(key, int);
	__type(value, struct task_ctx);
} task_ctxs SEC(".maps");

struct {
	__uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
	__uint(key_size, sizeof(u32));
	__uint(value_size, sizeof(u64));
	__uint(max_entries, DEVBOX_NR_STATS);
} stats SEC(".maps");

static __always_inline void stat_inc(u32 idx)
{
	u64 *cnt = bpf_map_lookup_elem(&stats, &idx);

	if (cnt)
		(*cnt)++;
}

static __always_inline struct task_ctx *lookup_tctx(struct task_struct *p)
{
	return bpf_task_storage_get(&task_ctxs, p, 0, 0);
}

static __always_inline u64 min64(u64 a, u64 b) { return a < b ? a : b; }
static __always_inline u64 max64(u64 a, u64 b) { return a > b ? a : b; }

static __always_inline bool vtime_before(u64 a, u64 b)
{
	return (s64)(a - b) < 0;
}

/* Inverse-weight scaling: weight 100 = 1.0, so heavier tasks are charged less. */
static __always_inline u64 scale_inv(u64 v, u32 w)
{
	return v * 100 / w;
}

/* scdr writes slice_us through sched_setattr; that sets se.custom_slice. */
static __always_inline bool has_latency_hint(struct task_struct *p)
{
	return p->se.custom_slice && p->se.slice < slice_ns * 2;
}

/* nice < 0 or a latency hint: scdr (or someone) asked for responsiveness. */
static __always_inline bool is_favoured(struct task_struct *p)
{
	return p->static_prio < NICE_0_PRIO || has_latency_hint(p);
}

#endif
