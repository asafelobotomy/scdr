CLANG    ?= clang
CC       ?= cc
BPFTOOL  ?= bpftool
PREFIX   ?= /usr/local
UNITDIR  ?= /etc/systemd/system
DOCDIR   ?= $(PREFIX)/share/doc/scx_devbox
BUILD    ?= build
ARCH     := $(shell uname -m | sed -e 's/x86_64/x86/' -e 's/aarch64/arm64/')

# CFLAGS/LDFLAGS from the environment (e.g. makepkg) apply to the loader only.
CFLAGS   ?= -O2 -g -Wall -Wextra
BPF_CFLAGS := -O2 -g -Wall -Wno-missing-declarations -target bpf -D__TARGET_ARCH_$(ARCH) \
              -mcpu=v3 -I$(BUILD) -Isrc

all: $(BUILD)/scx_devbox $(BUILD)/latbench

$(BUILD)/latbench: tools/latbench.c | $(BUILD)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< -o $@ $(LDFLAGS)

$(BUILD):
	mkdir -p $@

# CO-RE: the types come from the build machine's kernel, and libbpf relocates
# field offsets for the kernel it loads on. Needs a sched_ext kernel to build.
$(BUILD)/vmlinux.h: | $(BUILD)
	$(BPFTOOL) btf dump file /sys/kernel/btf/vmlinux format c > $@

$(BUILD)/devbox.bpf.o: src/devbox.bpf.c src/devbox.h $(BUILD)/vmlinux.h
	$(CLANG) $(BPF_CFLAGS) -c $< -o $@

$(BUILD)/devbox.bpf.skel.h: $(BUILD)/devbox.bpf.o
	$(BPFTOOL) gen skeleton $< name scx_devbox > $@

$(BUILD)/scx_devbox: src/devbox.c src/devbox.h $(BUILD)/devbox.bpf.skel.h
	$(CC) $(CPPFLAGS) $(CFLAGS) -I$(BUILD) -Isrc $< -o $@ $(LDFLAGS) -lbpf -lelf -lz

install: $(BUILD)/scx_devbox
	install -Dm755 $(BUILD)/scx_devbox $(DESTDIR)$(PREFIX)/bin/scx_devbox
	install -Dm644 README.md $(DESTDIR)$(DOCDIR)/README.md
	install -dm755 $(DESTDIR)$(UNITDIR)
	sed -e 's|@BINDIR@|$(PREFIX)/bin|' -e 's|@DOCDIR@|$(DOCDIR)|' \
		dist/scx_devbox.service > $(DESTDIR)$(UNITDIR)/scx_devbox.service
	chmod 644 $(DESTDIR)$(UNITDIR)/scx_devbox.service

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/scx_devbox \
	      $(DESTDIR)$(UNITDIR)/scx_devbox.service
	rm -rf $(DESTDIR)$(DOCDIR)

clean:
	rm -rf $(BUILD)

.PHONY: all install uninstall clean
