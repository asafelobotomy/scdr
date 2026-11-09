// SPDX-License-Identifier: GPL-2.0
/*
 * scx_devbox: a sched_ext scheduler for low-end machines doing code
 * development with LLM agent apps (Claude Desktop, Claude Code, ...).
 *
 * The workload it is shaped for:
 *   - a compositor and a few Electron/Chromium processes that must stay
 *     responsive while you read and type;
 *   - an agent that fires off many short tool processes (git, rg, sh, node,
 *     test runners) and waits on each of them;
 *   - CPU hogs (rustc, cc1, ld, qemu, rust-analyzer) that take every core;
 *   - not enough RAM, so kswapd and friends also need CPU at bad moments.
 *
 * Policy:
 *   - Two DSQs. INTERACTIVE is ordered by deadline (vruntime + expected
 *     burst), so tasks that run briefly and sleep go first. BATCH holds CPU
 *     hogs and is only served when INTERACTIVE is empty, with a starvation
 *     guard so builds still make progress.
 *   - A task is batch if scdr (or anyone) gave it SCHED_BATCH/SCHED_IDLE,
 *     or if its average run burst between sleeps exceeds batch_thresh_ns.
 *     Tasks with a latency hint, nice < 0, or PF_KTHREAD are never demoted
 *     by the heuristic.
 *   - scdr's per-profile slice_us (sched_attr custom slice) is honoured as
 *     the task's time slice and marks it latency-sensitive.
 *   - An interactive wakeup with no idle CPU preempts a batch task that has
 *     run at least batch_min_run_ns. Idle CPUs: kernel default pick.
 */
#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>
#include <bpf/bpf_core_read.h>
#include "devbox.h"

char _license[] SEC("license") = "GPL";

#define BPF_STRUCT_OPS(name, args...) \
	SEC("struct_ops/" #name) BPF_PROG(name, ##args)
#define BPF_STRUCT_OPS_SLEEPABLE(name, args...) \
	SEC("struct_ops.s/" #name) BPF_PROG(name, ##args)

#define INTERACTIVE_DSQ	0
#define BATCH_DSQ	1
#define MAX_CPUS	512	/* power of two, used as an index mask */

#define PF_KTHREAD	0x00200000
#define SCHED_BATCH	3
#define SCHED_IDLE	5
#define NICE_0_PRIO	120

/* Tunables, set by the loader before load. */
const volatile u64 slice_ns = 3000000ULL;	   /* interactive slice */
const volatile u64 slice_min_ns = 500000ULL;	   /* floor under load */
const volatile u64 batch_slice_ns = 12000000ULL;   /* long, cache-friendly */
const volatile u64 batch_thresh_ns = 8000000ULL;   /* avg burst => batch */
const volatile u64 batch_max_wait_ns = 60000000ULL; /* starvation guard */
const volatile u64 lag_ns = 20000000ULL;	   /* max sleep credit */
const volatile bool preempt_batch = true;
const volatile u64 batch_min_run_ns = 1000000ULL; /* before preemptible */
const volatile u32 nr_cpus = 1;

/* Global state. Racy plain accesses are fine: these are heuristics. */
u64 vtime_now;
u64 batch_served_at;
/* When the batch task on each CPU started running; 0 = not running batch. */
u64 cpu_batch_since[MAX_CPUS];

/* Exit info for the loader. */
s32 exit_kind;
char exit_reason[EXIT_MSG_LEN];
char exit_msg[EXIT_MSG_LEN];

struct task_ctx {
	u64 vruntime;	/* weighted runtime, our fairness clock */
	u64 avg_burst;	/* EWMA of runtime between sleeps */
	u64 burst;	/* runtime since the task last slept */
	u64 run_at;	/* when it last started running */
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

/* Inverse-weight scaling: nice 0 = 100, so heavier tasks are charged less. */
static __always_inline u64 scale_inv(u64 v, struct task_struct *p)
{
	u32 w = p->scx.weight ? p->scx.weight : 100;

	return v * 100 / w;
}

/* scdr writes slice_us through sched_setattr; that sets se.custom_slice. */
static __always_inline bool has_latency_hint(struct task_struct *p)
{
	return p->se.custom_slice && p->se.slice < slice_ns * 2;
}

static __always_inline bool is_protected(struct task_struct *p)
{
	return (p->flags & PF_KTHREAD) || p->static_prio < NICE_0_PRIO ||
	       has_latency_hint(p);
}

static bool classify_batch(struct task_struct *p, struct task_ctx *tctx)
{
	if (p->policy == SCHED_BATCH || p->policy == SCHED_IDLE)
		return true;
	if (is_protected(p))
		return false;
	/* Hysteresis: leave batch only once bursts fall well below threshold. */
	if (tctx->batch)
		return tctx->avg_burst > batch_thresh_ns / 2;
	return tctx->avg_burst > batch_thresh_ns;
}

static u64 task_slice(struct task_struct *p, bool batch)
{
	u64 queued, s;

	if (batch)
		return batch_slice_ns;

	if (has_latency_hint(p))
		return max64(p->se.slice, slice_min_ns / 2);

	/* Shrink the slice as the interactive queue grows. */
	queued = scx_bpf_dsq_nr_queued(INTERACTIVE_DSQ);
	s = slice_ns * nr_cpus / (nr_cpus + queued);
	return max64(s, slice_min_ns);
}

/* Find a CPU running a batch task that @p may run on, preferring @prefer. */
static __always_inline bool batch_preemptible(struct task_struct *p, u32 cpu,
					      u64 now)
{
	u64 since = cpu_batch_since[cpu & (MAX_CPUS - 1)];

	/* Let a batch task keep at least batch_min_run_ns: no thrashing. */
	return since && now - since >= batch_min_run_ns &&
	       bpf_cpumask_test_cpu(cpu, p->cpus_ptr);
}

static s32 find_batch_cpu(struct task_struct *p, s32 prefer)
{
	u64 now = bpf_ktime_get_ns();
	u32 i;

	if (prefer >= 0 && prefer < MAX_CPUS &&
	    batch_preemptible(p, prefer, now))
		return prefer;

	for (i = 0; i < MAX_CPUS && i < nr_cpus; i++) {
		if (batch_preemptible(p, i, now))
			return i;
	}
	return -1;
}

s32 BPF_STRUCT_OPS(devbox_select_cpu, struct task_struct *p, s32 prev_cpu,
		   u64 wake_flags)
{
	struct task_ctx *tctx;
	bool is_idle = false;
	s32 cpu;

	cpu = scx_bpf_select_cpu_dfl(p, prev_cpu, wake_flags, &is_idle);
	if (is_idle) {
		tctx = lookup_tctx(p);
		bool batch = tctx ? tctx->batch : false;

		scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, task_slice(p, batch), 0);
		stat_inc(DST_DIRECT_IDLE);
	}
	return cpu;
}

void BPF_STRUCT_OPS(devbox_enqueue, struct task_struct *p, u64 enq_flags)
{
	struct task_ctx *tctx;
	u64 vruntime, floor, deadline;
	s32 cpu;

	/*
	 * Per-CPU kthreads (kworkers, ksoftirqd, ...) can only run here and
	 * usually unblock someone else: run them next on their CPU.
	 */
	if ((p->flags & PF_KTHREAD) && p->nr_cpus_allowed == 1) {
		scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, slice_ns, enq_flags);
		stat_inc(DST_KTHREAD);
		return;
	}

	tctx = lookup_tctx(p);
	if (!tctx) {
		scx_bpf_dsq_insert(p, SCX_DSQ_GLOBAL, slice_ns, enq_flags);
		return;
	}

	tctx->batch = classify_batch(p, tctx);
	if (has_latency_hint(p))
		stat_inc(DST_HINTED);

	/* Bound the credit a long sleeper can bring back. */
	vruntime = tctx->vruntime;
	floor = vtime_now - lag_ns;
	if (vtime_before(vruntime, floor))
		vruntime = floor;
	tctx->vruntime = vruntime;

	/* Expected burst pushes the deadline out: short runners go first. */
	deadline = vruntime + scale_inv(min64(tctx->avg_burst, lag_ns), p);

	if (tctx->batch) {
		if (!scx_bpf_dsq_nr_queued(BATCH_DSQ))
			batch_served_at = bpf_ktime_get_ns();
		scx_bpf_dsq_insert_vtime(p, BATCH_DSQ, task_slice(p, true),
					 deadline, enq_flags);
		stat_inc(DST_BATCH);
	} else {
		scx_bpf_dsq_insert_vtime(p, INTERACTIVE_DSQ,
					 task_slice(p, false), deadline,
					 enq_flags);
		stat_inc(DST_INTERACTIVE);
	}

	/* Get a CPU onto it: an idle one first, else evict a batch task. */
	cpu = scx_bpf_pick_idle_cpu(p->cpus_ptr, 0);
	if (cpu >= 0) {
		scx_bpf_kick_cpu(cpu, SCX_KICK_IDLE);
		stat_inc(DST_IDLE_KICK);
		return;
	}
	if (preempt_batch && !tctx->batch) {
		cpu = find_batch_cpu(p, scx_bpf_task_cpu(p));
		if (cpu >= 0) {
			cpu_batch_since[cpu & (MAX_CPUS - 1)] = 0;
			scx_bpf_kick_cpu(cpu, SCX_KICK_PREEMPT);
			stat_inc(DST_PREEMPT_KICK);
		}
	}
}

void BPF_STRUCT_OPS(devbox_dispatch, s32 cpu, struct task_struct *prev)
{
	u64 now = bpf_ktime_get_ns();

	if (scx_bpf_dsq_nr_queued(BATCH_DSQ) &&
	    now - batch_served_at > batch_max_wait_ns) {
		if (scx_bpf_dsq_move_to_local(BATCH_DSQ)) {
			batch_served_at = now;
			stat_inc(DST_STARVE_RESCUE);
			return;
		}
	}

	if (scx_bpf_dsq_move_to_local(INTERACTIVE_DSQ))
		return;

	if (scx_bpf_dsq_move_to_local(BATCH_DSQ)) {
		batch_served_at = now;
		return;
	}

	/* Nothing else wants this CPU: let prev keep going. */
	if (prev && (prev->scx.flags & SCX_TASK_QUEUED)) {
		struct task_ctx *tctx = lookup_tctx(prev);

		prev->scx.slice = task_slice(prev, tctx ? tctx->batch : false);
	}
}

void BPF_STRUCT_OPS(devbox_running, struct task_struct *p)
{
	struct task_ctx *tctx = lookup_tctx(p);
	s32 cpu = scx_bpf_task_cpu(p);

	if (!tctx)
		return;
	tctx->run_at = bpf_ktime_get_ns();
	if (vtime_before(vtime_now, tctx->vruntime))
		vtime_now = tctx->vruntime;
	if (cpu >= 0 && cpu < MAX_CPUS)
		cpu_batch_since[cpu & (MAX_CPUS - 1)] = tctx->batch ? tctx->run_at : 0;
	if (tctx->batch)
		batch_served_at = tctx->run_at;
}

void BPF_STRUCT_OPS(devbox_stopping, struct task_struct *p, bool runnable)
{
	struct task_ctx *tctx = lookup_tctx(p);
	s32 cpu = scx_bpf_task_cpu(p);
	u64 used;

	if (cpu >= 0 && cpu < MAX_CPUS)
		cpu_batch_since[cpu & (MAX_CPUS - 1)] = 0;
	if (!tctx)
		return;

	used = bpf_ktime_get_ns() - tctx->run_at;
	tctx->vruntime += scale_inv(used, p);
	tctx->burst = min64(tctx->burst + used, 10 * batch_thresh_ns);

	/* A hog that never sleeps must still be recognised. */
	if (runnable && tctx->burst > tctx->avg_burst)
		tctx->avg_burst = (tctx->avg_burst * 3 + tctx->burst) / 4;
}

void BPF_STRUCT_OPS(devbox_quiescent, struct task_struct *p, u64 deq_flags)
{
	struct task_ctx *tctx = lookup_tctx(p);

	if (!tctx)
		return;
	/* Went to sleep: fold this burst into the average. */
	tctx->avg_burst = (tctx->avg_burst * 3 + tctx->burst) / 4;
	tctx->burst = 0;
}

void BPF_STRUCT_OPS(devbox_enable, struct task_struct *p)
{
	struct task_ctx *tctx = lookup_tctx(p);

	if (tctx)
		tctx->vruntime = vtime_now;
}

s32 BPF_STRUCT_OPS(devbox_init_task, struct task_struct *p,
		   struct scx_init_task_args *args)
{
	if (!bpf_task_storage_get(&task_ctxs, p, 0,
				  BPF_LOCAL_STORAGE_GET_F_CREATE))
		return -12; /* -ENOMEM */
	return 0;
}

s32 BPF_STRUCT_OPS_SLEEPABLE(devbox_init)
{
	s32 ret;

	batch_served_at = bpf_ktime_get_ns();
	ret = scx_bpf_create_dsq(INTERACTIVE_DSQ, -1);
	if (ret)
		return ret;
	return scx_bpf_create_dsq(BATCH_DSQ, -1);
}

void BPF_STRUCT_OPS(devbox_exit, struct scx_exit_info *ei)
{
	bpf_probe_read_kernel_str(exit_reason, sizeof(exit_reason), ei->reason);
	bpf_probe_read_kernel_str(exit_msg, sizeof(exit_msg), ei->msg);
	exit_kind = ei->kind;
}

SEC(".struct_ops.link")
struct sched_ext_ops devbox_ops = {
	.select_cpu	= (void *)devbox_select_cpu,
	.enqueue	= (void *)devbox_enqueue,
	.dispatch	= (void *)devbox_dispatch,
	.running	= (void *)devbox_running,
	.stopping	= (void *)devbox_stopping,
	.quiescent	= (void *)devbox_quiescent,
	.enable		= (void *)devbox_enable,
	.init_task	= (void *)devbox_init_task,
	.init		= (void *)devbox_init,
	.exit		= (void *)devbox_exit,
	.timeout_ms	= 5000,
	.name		= "devbox",
};
