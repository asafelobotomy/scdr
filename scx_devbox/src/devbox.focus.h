/* SPDX-License-Identifier: GPL-2.0 */
/*
 * Focus hints from scdr, for devbox.bpf.c.
 *
 * scdr knows which app has the focus (its session agent follows the window
 * manager) and where each user's apps run (app.slice). It writes both to
 * /run/scx_devbox/hints; the loader turns the cgroup paths into ids and
 * stores them here, in .bss, which userspace maps directly, then bumps
 * hints_gen. A focus change is therefore one file write, instead of scdr
 * renicing every thread of the old and new app and asking systemd to
 * change two units' CPUWeight.
 *
 * Tasks in a focused cgroup (or below it) are treated like scdr's latency
 * hints: shielded from batch demotion, sorted ahead by focus_bonus_ns and
 * given the frequency floor. Tasks in a user's app.slice but not in the
 * focused app are weighted down by bg_factor_pct, and only while the CPUs
 * are contended (more queued tasks than CPUs), measured here at enqueue
 * rather than waiting on a pressure average.
 */
#ifndef __DEVBOX_FOCUS_H
#define __DEVBOX_FOCUS_H

enum focus_class {
	FC_NONE,
	FC_FOCUS,	/* in a focused app */
	FC_APP_BG,	/* in a user's apps, not focused */
};

const volatile u64 focus_bonus_ns = 3000000ULL;
const volatile u32 bg_factor_pct = 50;

/* Written by the loader; zero ids are unused slots. */
u64 focus_cgid[MAX_HINTS];
u32 focus_level[MAX_HINTS];
u64 apps_cgid[MAX_HINTS];
u32 apps_level[MAX_HINTS];
u32 hints_gen;

/* Whether @cg is the cgroup @id (at @level) or below it. */
static __always_inline bool cg_under(struct cgroup *cg, u64 id, u32 level)
{
	struct cgroup *anc;
	bool hit;

	if (!id || cg->level < (int)level)
		return false;
	anc = bpf_cgroup_ancestor(cg, level);
	if (!anc)
		return false;
	hit = anc->kn->id == id;
	bpf_cgroup_release(anc);
	return hit;
}

static u8 compute_focus_class(struct task_struct *p)
{
	struct cgroup *cg = scx_bpf_task_cgroup(p);
	u8 fc = FC_NONE;
	u32 i;

	if (!cg)
		return FC_NONE;
	for (i = 0; i < MAX_HINTS; i++) {
		if (cg_under(cg, focus_cgid[i], focus_level[i])) {
			fc = FC_FOCUS;
			goto out;
		}
	}
	for (i = 0; i < MAX_HINTS; i++) {
		if (cg_under(cg, apps_cgid[i], apps_level[i])) {
			fc = FC_APP_BG;
			break;
		}
	}
out:
	bpf_cgroup_release(cg);
	return fc;
}

/* Cached in task_ctx; recomputed when the hints or the task's cgroup change. */
static __always_inline u8 focus_class(struct task_struct *p, struct task_ctx *tctx)
{
	u32 gen = hints_gen;

	if (tctx->fgen != gen + 1) {
		tctx->fclass = gen ? compute_focus_class(p) : FC_NONE;
		tctx->fgen = gen + 1;
	}
	return tctx->fclass;
}

/* More runnable tasks than CPUs: someone is waiting. */
static __always_inline bool contended(void)
{
	return scx_bpf_dsq_nr_queued(INTERACTIVE_DSQ) +
	       scx_bpf_dsq_nr_queued(BATCH_DSQ) >= nr_cpus;
}

/* @weight adjusted for focus; @deadline pulled forward for focused tasks. */
static __always_inline u32 focus_weight(u8 fc, u32 weight, bool count)
{
	if (fc == FC_APP_BG && contended()) {
		if (count)
			stat_inc(DST_BG_DAMPED);
		weight = weight * bg_factor_pct / 100;
		return weight ? weight : 1;
	}
	return weight;
}

static __always_inline u64 focus_deadline(u8 fc, u64 deadline)
{
	if (fc != FC_FOCUS)
		return deadline;
	stat_inc(DST_FOCUS);
	return deadline - focus_bonus_ns;
}

#endif
