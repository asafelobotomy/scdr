/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scdr's focus hints, for the loader (see devbox.focus.h for what the
 * scheduler does with them).
 *
 * scdr writes /run/scdr/devbox-hints (in its own runtime directory, the
 * one place its sandbox lets it write; atomically, by rename), one hint per
 * line:
 *
 *   focus <cgroup path>   the focused app's unit, e.g. /user.slice/.../app.slice/app-foo-12.scope
 *   apps <cgroup path>    a user's app.slice
 *
 * The loader resolves each path to its cgroup id (the inode number of its
 * cgroupfs directory, which is what the kernel uses as the id), stores the
 * ids and levels in the scheduler's .bss and bumps hints_gen so tasks
 * recompute their class. /run/scx_devbox/api, which only needs to be
 * readable by scdr, tells it that this loader takes hints.
 */
#ifndef __DEVBOX_HINTS_H
#define __DEVBOX_HINTS_H

#include <poll.h>
#include <sys/inotify.h>
#include <sys/stat.h>

#define HINTS_DIR	"/run/scdr"
#define HINTS_NAME	"devbox-hints"
#define HINTS_FILE	HINTS_DIR "/" HINTS_NAME
#define API_DIR		"/run/scx_devbox"
#define HINTS_API	API_DIR "/api"
#define HINTS_API_VERSION "1\n"

/* Depth of a cgroup path: "/a/b.slice/c.scope" is level 3. */
static unsigned int cgroup_level(const char *path)
{
	unsigned int level = 0;

	for (const char *p = path; *p; p++) {
		if (*p != '/' && (p == path || p[-1] == '/'))
			level++;
	}
	return level;
}

static bool cgroup_id(const char *path, __u64 *id)
{
	char full[4096];
	struct stat st;

	if (path[0] != '/' || strstr(path, "/../") ||
	    snprintf(full, sizeof(full), "/sys/fs/cgroup%s", path) >= (int)sizeof(full) ||
	    stat(full, &st) || !S_ISDIR(st.st_mode))
		return false;
	*id = st.st_ino;
	return true;
}

/* Reads the hints file (missing = no hints) into the scheduler. */
static void hints_load(struct scx_devbox *skel, bool verbose_log)
{
	__u64 focus[MAX_HINTS] = { 0 }, apps[MAX_HINTS] = { 0 };
	__u32 focus_lv[MAX_HINTS] = { 0 }, apps_lv[MAX_HINTS] = { 0 };
	unsigned int nf = 0, na = 0;
	char line[4200];
	FILE *f = fopen(HINTS_FILE, "r");

	while (f && fgets(line, sizeof(line), f)) {
		char *path;
		__u64 id;

		line[strcspn(line, "\n")] = 0;
		path = strchr(line, ' ');
		if (!path)
			continue;
		*path++ = 0;
		if (!cgroup_id(path, &id))
			continue;
		if (!strcmp(line, "focus") && nf < MAX_HINTS) {
			focus[nf] = id;
			focus_lv[nf++] = cgroup_level(path);
		} else if (!strcmp(line, "apps") && na < MAX_HINTS) {
			apps[na] = id;
			apps_lv[na++] = cgroup_level(path);
		}
	}
	if (f)
		fclose(f);

	for (int i = 0; i < MAX_HINTS; i++) {
		skel->bss->focus_cgid[i] = focus[i];
		skel->bss->focus_level[i] = focus_lv[i];
		skel->bss->apps_cgid[i] = apps[i];
		skel->bss->apps_level[i] = apps_lv[i];
	}
	/* Publish after the ids: tasks recompute their class on the new gen. */
	__atomic_add_fetch(&skel->bss->hints_gen, 1, __ATOMIC_RELEASE);
	if (verbose_log)
		fprintf(stderr, "hints: %u focused, %u app slices\n", nf, na);
}

/* Tells scdr that this loader takes hints. */
static void hints_announce(void)
{
	FILE *api;

	mkdir(API_DIR, 0755);
	api = fopen(HINTS_API, "w");
	if (api) {
		fputs(HINTS_API_VERSION, api);
		fclose(api);
	}
}

/* Watches scdr's runtime directory; -1 while it doesn't exist (yet). */
static int hints_watch(void)
{
	int fd = inotify_init1(IN_NONBLOCK | IN_CLOEXEC);

	if (fd >= 0 && inotify_add_watch(fd, HINTS_DIR,
					 IN_CLOSE_WRITE | IN_MOVED_TO | IN_DELETE |
					 IN_DELETE_SELF | IN_MOVE_SELF) < 0) {
		close(fd);
		fd = -1;
	}
	return fd;
}

/*
 * Waits up to @ms for a change to the hints file; true if there was one.
 * If the directory itself goes (scdr stopped for good), closes *@fd so the
 * caller watches again once it is back.
 */
static bool hints_wait(int *fdp, int ms)
{
	int fd = *fdp;
	char buf[4096] __attribute__((aligned(__alignof__(struct inotify_event))));
	struct pollfd pfd = { .fd = fd, .events = POLLIN };
	bool changed = false;
	ssize_t n;

	if (fd < 0) {
		usleep(ms * 1000);
		return false;
	}
	if (poll(&pfd, 1, ms) <= 0)
		return false;
	while ((n = read(fd, buf, sizeof(buf))) > 0) {
		for (char *p = buf; p < buf + n;) {
			struct inotify_event *ev = (struct inotify_event *)p;

			if (ev->len && !strcmp(ev->name, HINTS_NAME))
				changed = true;
			if (ev->mask & (IN_IGNORED | IN_DELETE_SELF | IN_MOVE_SELF)) {
				close(fd);
				*fdp = -1;
				return true;
			}
			p += sizeof(*ev) + ev->len;
		}
	}
	return changed;
}

#endif
