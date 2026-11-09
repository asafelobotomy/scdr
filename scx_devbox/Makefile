CLANG    ?= clang
CC       ?= cc
BPFTOOL  ?= bpftool
PREFIX   ?= /usr/local
ARCH     := $(shell uname -m | sed -e 's/x86_64/x86/' -e 's/aarch64/arm64/')
BUILD    := build

CFLAGS   ?= -O2 -g -Wall -Wextra
BPF_CFLAGS := -O2 -g -Wall -Wno-missing-declarations -target bpf -D__TARGET_ARCH_$(ARCH) \
              -mcpu=v3 -I$(BUILD) -Isrc

all: $(BUILD)/scx_devbox $(BUILD)/latbench

$(BUILD)/latbench: tools/latbench.c | $(BUILD)
	$(CC) $(CFLAGS) $< -o $@

$(BUILD):
	mkdir -p $@

$(BUILD)/vmlinux.h: | $(BUILD)
	$(BPFTOOL) btf dump file /sys/kernel/btf/vmlinux format c > $@

$(BUILD)/devbox.bpf.o: src/devbox.bpf.c src/devbox.h $(BUILD)/vmlinux.h
	$(CLANG) $(BPF_CFLAGS) -c $< -o $@

$(BUILD)/devbox.bpf.skel.h: $(BUILD)/devbox.bpf.o
	$(BPFTOOL) gen skeleton $< name scx_devbox > $@

$(BUILD)/scx_devbox: src/devbox.c src/devbox.h $(BUILD)/devbox.bpf.skel.h
	$(CC) $(CFLAGS) -I$(BUILD) -Isrc $< -o $@ -lbpf -lelf -lz

install: $(BUILD)/scx_devbox
	install -Dm755 $(BUILD)/scx_devbox $(DESTDIR)$(PREFIX)/bin/scx_devbox
	install -Dm644 dist/scx_devbox.service \
		$(DESTDIR)/etc/systemd/system/scx_devbox.service
	install -Dm644 README.md $(DESTDIR)$(PREFIX)/share/doc/scx_devbox/README.md
	sed -i 's|@BINDIR@|$(PREFIX)/bin|' \
		$(DESTDIR)/etc/systemd/system/scx_devbox.service

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/scx_devbox \
	      $(DESTDIR)/etc/systemd/system/scx_devbox.service
	rm -rf $(DESTDIR)$(PREFIX)/share/doc/scx_devbox

clean:
	rm -rf $(BUILD)

.PHONY: all install uninstall clean
