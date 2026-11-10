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
 *     Tasks with a latency hint or nice < 0 get hint_thresh_mult times the
 *     threshold (scdr's foreground role covers a whole app, hogs included);
 *     kthreads are never demoted.
 *   - scdr's per-profile slice_us (sched_attr custom slice) is honoured as
 *     the task's time slice and marks it latency-sensitive.
 *   - cgroup cpu.weight is honoured: a task is charged vruntime at its nice
 *     weight times its cgroup's flattened weight (devbox.cgroup.h).
 *   - An interactive wakeup with no idle CPU preempts a batch task that has
 *     run at least batch_min_run_ns. Idle CPUs: kernel default pick.
 */
#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>
#include <bpf/bpf_core_read.h>
#include "devbox.h"
#include "devbox.bpf.h"
#include "devbox.cgroup.h"
#include "devbox.perf.h"

char _license[] SEC("license") = "GPL";

static bool classify_batch(struct task_struct *p, struct task_ctx *tctx)
{
	u64 thresh = batch_thresh_ns;
	bool batch;

	if (p->policy == SCHED_BATCH || p->policy == SCHED_IDLE)
		return true;
	if (p->flags & PF_KTHREAD)
		return false;
	if (is_favoured(p))
		thresh *= hint_thresh_mult;
	/* Hysteresis: leave batch only once bursts fall well below threshold. */
	if (tctx->batch)
		thresh /= 2;
	batch = tctx->avg_burst > thresh;
	if (batch && is_favoured(p))
		stat_inc(DST_HINT_DEMOTED);
	return batch;
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

static __always_inline bool batch_preemptible(struct task_struct *p, u32 cpu,
					      u64 now)
{
	u64 since = cpu_batch_since[CPU_IDX(cpu)];

	/* Let a batch task keep at least batch_min_run_ns: no thrashing. */
	return since && now - since >= batch_min_run_ns &&
	       bpf_cpumask_test_cpu(cpu, p->cpus_ptr);
}

/* Find a CPU running a batch task that @p may run on, preferring @prefer. */
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

	/*
	 * scx_bpf_select_cpu_dfl() is deprecated upstream in favour of
	 * scx_bpf_select_cpu_and(); keep the old call for kernels without it.
	 */
	if (bpf_ksym_exists(scx_bpf_select_cpu_and)) {
		cpu = scx_bpf_select_cpu_and(p, prev_cpu, wake_flags, p->cpus_ptr, 0);
		is_idle = cpu >= 0;
		if (!is_idle)
			cpu = prev_cpu;
	} else {
		cpu = scx_bpf_select_cpu_dfl(p, prev_cpu, wake_flags, &is_idle);
	}
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
	u64 vruntime, clock, deadline;
	u32 weight;
	bool was_batch;
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

	was_batch = tctx->batch;
	tctx->batch = classify_batch(p, tctx);
	if (has_latency_hint(p))
		stat_inc(DST_HINTED);

	/*
	 * Changing class means changing clocks: start level with the new one.
	 * Otherwise bound the credit a long sleeper brings back, on wakeup
	 * only: a task put back after its slice hasn't slept, and flooring it
	 * against a clock that low-weight tasks race ahead would erase the
	 * lead its weight earned (nice 0 vs 10 got 5:1 instead of ~9:1).
	 */
	clock = tctx->batch ? vtime_now[BATCH_DSQ] : vtime_now[INTERACTIVE_DSQ];
	vruntime = tctx->vruntime;
	if (tctx->batch != was_batch)
		vruntime = clock;
	else if ((enq_flags & SCX_ENQ_WAKEUP) && vtime_before(vruntime, clock - lag_ns))
		vruntime = clock - lag_ns;
	tctx->vruntime = vruntime;

	weight = eff_weight(p, tctx);
	if (tctx->cgw != CGW_ONE)
		stat_inc(DST_CGROUP_WEIGHTED);

	/* Expected burst pushes the deadline out: short runners go first. */
	deadline = vruntime + scale_inv(min64(tctx->avg_burst, lag_ns), weight);

	if (tctx->batch) {
		if (!scx_bpf_dsq_nr_queued(BATCH_DSQ))
			batch_nonempty_since = bpf_ktime_get_ns();
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
			cpu_batch_since[CPU_IDX(cpu)] = 0;
			scx_bpf_kick_cpu(cpu, SCX_KICK_PREEMPT);
			stat_inc(DST_PREEMPT_KICK);
		}
	}
}

void BPF_STRUCT_OPS(devbox_dispatch, s32 cpu, struct task_struct *prev)
{
	u64 now = bpf_ktime_get_ns();
	u32 idx = CPU_IDX(cpu);
	u64 last = max64(cpu_batch_served[idx], batch_nonempty_since);

	/*
	 * Starvation guard, per CPU: each CPU serves the batch queue at least
	 * every batch_max_wait_ns, so N batch tasks wait about
	 * N / nr_cpus * batch_max_wait_ns at worst, well inside the watchdog.
	 */
	if (now - last > batch_max_wait_ns && scx_bpf_dsq_nr_queued(BATCH_DSQ) &&
	    scx_bpf_dsq_move_to_local(BATCH_DSQ)) {
		cpu_batch_served[idx] = now;
		stat_inc(DST_STARVE_RESCUE);
		return;
	}

	if (scx_bpf_dsq_move_to_local(INTERACTIVE_DSQ))
		return;

	if (scx_bpf_dsq_move_to_local(BATCH_DSQ)) {
		cpu_batch_served[idx] = now;
		return;
	}

	/* Nothing else wants this CPU: let prev keep going. */
	if (prev && (prev->scx.flags & SCX_TASK_QUEUED)) {
		struct task_ctx *tctx = lookup_tctx(prev);

		scx_bpf_task_set_slice(prev, task_slice(prev, tctx ? tctx->batch : false));
	}
}

void BPF_STRUCT_OPS(devbox_running, struct task_struct *p)
{
	struct task_ctx *tctx = lookup_tctx(p);
	u32 idx = CPU_IDX(scx_bpf_task_cpu(p));

	if (!tctx)
		return;
	tctx->run_at = bpf_ktime_get_ns();
	perf_running(scx_bpf_task_cpu(p), tctx->run_at);
	if (tctx->batch) {
		if (vtime_before(vtime_now[BATCH_DSQ], tctx->vruntime))
			vtime_now[BATCH_DSQ] = tctx->vruntime;
		cpu_batch_since[idx] = tctx->run_at;
		cpu_batch_served[idx] = tctx->run_at;
	} else {
		if (vtime_before(vtime_now[INTERACTIVE_DSQ], tctx->vruntime))
			vtime_now[INTERACTIVE_DSQ] = tctx->vruntime;
		cpu_batch_since[idx] = 0;
	}
}

void BPF_STRUCT_OPS(devbox_stopping, struct task_struct *p, bool runnable)
{
	struct task_ctx *tctx = lookup_tctx(p);
	u64 used;

	cpu_batch_since[CPU_IDX(scx_bpf_task_cpu(p))] = 0;
	perf_stopping(scx_bpf_task_cpu(p), bpf_ktime_get_ns());
	if (!tctx)
		return;

	used = bpf_ktime_get_ns() - tctx->run_at;
	tctx->vruntime += scale_inv(used, eff_weight(p, tctx));
	tctx->burst = min64(tctx->burst + used,
			    4 * hint_thresh_mult * batch_thresh_ns);

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
		tctx->vruntime = vtime_now[INTERACTIVE_DSQ];
}

s32 BPF_STRUCT_OPS(devbox_init_task, struct task_struct *p,
		   struct scx_init_task_args *args)
{
	struct task_ctx *tctx;

	tctx = bpf_task_storage_get(&task_ctxs, p, 0,
				    BPF_LOCAL_STORAGE_GET_F_CREATE);
	if (!tctx)
		return -12; /* -ENOMEM */
	tctx->cgw = args->cgroup ? cgrp_factor(args->cgroup) : CGW_ONE;
	tctx->cgw_gen = cgw_gen;
	return 0;
}

/*
 * Implementing these keeps the scheduler loaded across CPU hotplug (suspend,
 * SMT toggles); without them the kernel unloads it on the first event.
 */
void BPF_STRUCT_OPS(devbox_cpu_online, s32 cpu)
{
	cpu_batch_since[CPU_IDX(cpu)] = 0;
}

void BPF_STRUCT_OPS(devbox_cpu_offline, s32 cpu)
{
	cpu_batch_since[CPU_IDX(cpu)] = 0;
}

s32 BPF_STRUCT_OPS_SLEEPABLE(devbox_init)
{
	s32 ret;

	ret = scx_bpf_create_dsq(INTERACTIVE_DSQ, -1);
	if (ret)
		return ret;
	return scx_bpf_create_dsq(BATCH_DSQ, -1);
}

void BPF_STRUCT_OPS(devbox_exit, struct scx_exit_info *ei)
{
	bpf_probe_read_kernel_str(exit_reason, sizeof(exit_reason), ei->reason);
	bpf_probe_read_kernel_str(exit_msg, sizeof(exit_msg), ei->msg);
	exit_code = ei->exit_code;
	exit_kind = ei->kind;
}

void BPF_STRUCT_OPS(devbox_tick, struct task_struct *p)
{
	perf_tick(scx_bpf_task_cpu(p), p, bpf_ktime_get_ns());
}

SEC(".struct_ops.link")
struct sched_ext_ops devbox_ops = {
	.select_cpu	= (void *)devbox_select_cpu,
	.enqueue	= (void *)devbox_enqueue,
	.dispatch	= (void *)devbox_dispatch,
	.running	= (void *)devbox_running,
	.stopping	= (void *)devbox_stopping,
	.tick		= (void *)devbox_tick,
	.quiescent	= (void *)devbox_quiescent,
	.enable		= (void *)devbox_enable,
	.init_task	= (void *)devbox_init_task,
	.cgroup_init	= (void *)devbox_cgroup_init,
	.cgroup_set_weight = (void *)devbox_cgroup_set_weight,
	.cgroup_move	= (void *)devbox_cgroup_move,
	.cpu_online	= (void *)devbox_cpu_online,
	.cpu_offline	= (void *)devbox_cpu_offline,
	.init		= (void *)devbox_init,
	.exit		= (void *)devbox_exit,
	.timeout_ms	= 10000,
	.name		= "devbox",
};
