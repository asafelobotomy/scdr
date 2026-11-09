/* SPDX-License-Identifier: GPL-2.0 */
/*
 * cgroup CPU weights (systemd CPUWeight= / cpu.weight) for devbox.bpf.c.
 *
 * Under sched_ext the kernel hands cgroup weights to the scheduler instead of
 * applying them. Each cgroup stores a flattened factor, the product of
 * (weight / 100) from the root down, in CGW_SHIFT fixed point. A task is
 * charged vruntime at (task weight x cgroup factor), so a CPUWeight=400 unit
 * accrues vruntime 4x slower and a CPUWeight=1 unit 100x faster.
 *
 * Approximation: a factor is computed from the parent's factor when the
 * cgroup is created. Changing a cgroup's weight updates that cgroup only,
 * its existing descendants keep the factor they were created with. Weights are
 * also treated as absolute (relative to the default 100) rather than
 * relative to siblings. scdr sets weights on leaf-ish units, so both are fine.
 */
#ifndef __DEVBOX_CGROUP_H
#define __DEVBOX_CGROUP_H

#define CGW_SHIFT	10
#define CGW_ONE		(1U << CGW_SHIFT)
#define CGW_MIN		1U			/* ~1/1000: never exactly zero */
#define CGW_MAX		(100U << CGW_SHIFT)	/* 10000/100 */
#define EFF_W_MIN	1U
#define EFF_W_MAX	10000U			/* kernel's weight range */

struct cgrp_ctx {
	u32 factor;	/* flattened weight / 100, CGW_SHIFT fixed point */
};

struct {
	__uint(type, BPF_MAP_TYPE_CGRP_STORAGE);
	__uint(map_flags, BPF_F_NO_PREALLOC);
	__type(key, int);
	__type(value, struct cgrp_ctx);
} cgrp_ctxs SEC(".maps");

/* Bumped on every weight change; cached task factors older than it refresh. */
u32 cgw_gen;

static __always_inline u32 cgw_clamp(u64 f)
{
	return f < CGW_MIN ? CGW_MIN : f > CGW_MAX ? CGW_MAX : (u32)f;
}

/* Factor stored for @cgrp; CGW_ONE if it has none (e.g. the root). */
static __always_inline u32 cgrp_factor(struct cgroup *cgrp)
{
	struct cgrp_ctx *c = bpf_cgrp_storage_get(&cgrp_ctxs, cgrp, 0, 0);

	return c ? c->factor : CGW_ONE;
}

/* Fresh factor for a task's cgroup. scx_bpf_task_cgroup() acquires a ref. */
static __always_inline u32 task_cgrp_factor(struct task_struct *p)
{
	struct cgroup *cgrp = scx_bpf_task_cgroup(p);
	u32 f = CGW_ONE;

	if (cgrp) {
		f = cgrp_factor(cgrp);
		bpf_cgroup_release(cgrp);
	}
	return f;
}

/* Cached factor, refreshed if any cgroup weight changed since. */
static __always_inline u32 tctx_cgw(struct task_struct *p, struct task_ctx *tctx)
{
	u32 gen = cgw_gen;

	if (tctx->cgw_gen != gen) {
		tctx->cgw = task_cgrp_factor(p);
		tctx->cgw_gen = gen;
	}
	return tctx->cgw;
}

/* Task weight (nice) combined with its cgroup factor, clamped. */
static __always_inline u32 eff_weight(struct task_struct *p, struct task_ctx *tctx)
{
	u64 w = p->scx.weight ? p->scx.weight : 100;

	w = w * tctx_cgw(p, tctx) >> CGW_SHIFT;
	return w < EFF_W_MIN ? EFF_W_MIN : w > EFF_W_MAX ? EFF_W_MAX : (u32)w;
}

static __always_inline void cgrp_set_factor(struct cgroup *cgrp, u32 weight)
{
	struct cgrp_ctx *c = bpf_cgrp_storage_get(&cgrp_ctxs, cgrp, 0,
						  BPF_LOCAL_STORAGE_GET_F_CREATE);
	u32 parent = CGW_ONE;

	if (!c)
		return;
	if (cgrp->level > 0) {
		struct cgroup *par = bpf_cgroup_ancestor(cgrp, cgrp->level - 1);

		if (par) {
			parent = cgrp_factor(par);
			bpf_cgroup_release(par);
		}
	}
	c->factor = cgw_clamp((u64)parent * weight / 100);
}

s32 BPF_STRUCT_OPS(devbox_cgroup_init, struct cgroup *cgrp,
		   struct scx_cgroup_init_args *args)
{
	cgrp_set_factor(cgrp, args->weight);
	return 0;
}

void BPF_STRUCT_OPS(devbox_cgroup_set_weight, struct cgroup *cgrp, u32 weight)
{
	cgrp_set_factor(cgrp, weight);
	__sync_fetch_and_add(&cgw_gen, 1);
}

void BPF_STRUCT_OPS(devbox_cgroup_move, struct task_struct *p,
		    struct cgroup *from, struct cgroup *to)
{
	struct task_ctx *tctx = lookup_tctx(p);

	if (!tctx)
		return;
	tctx->cgw = cgrp_factor(to);
	tctx->cgw_gen = cgw_gen;
}

#endif
