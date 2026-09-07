# Envoltorio fino sobre `cargo` + instalación estilo GNU (spec 0002 M1).
#
# La compilación la hace `cargo`; este Makefile solo orquesta `install` /
# `uninstall` con PREFIX/DESTDIR para empaquetadores y da atajos de
# conveniencia (`make`, `make test`, `make debug`).
#
#   make                       # cargo build --release --workspace
#   make install               # PREFIX=/usr/local (usa sudo)
#   make install PREFIX=$HOME/.local
#   make install DESTDIR=/tmp/pkg PREFIX=/usr CHANNEL=deb   # empaquetado
#   make uninstall
#   make dist                  # tarball reproducible desde git

CARGO   ?= cargo
PREFIX  ?= /usr/local
DESTDIR ?=

BINDIR     ?= $(PREFIX)/bin
DATADIR    ?= $(PREFIX)/share
MANDIR     ?= $(DATADIR)/man/man1
LIBDIR     ?= $(PREFIX)/lib
APPDIR     ?= $(DATADIR)/applications
ICONDIR    ?= $(DATADIR)/icons/hicolor
PKGDATADIR ?= $(DATADIR)/wayvpet

# Unidad systemd de usuario: en un prefijo de sistema va a lib/systemd/user; en
# un prefijo de HOME (~/.local) systemd la busca en $XDG_DATA_HOME/systemd/user
# = share/systemd/user, no en lib/.
ifneq ($(filter /usr /usr/local,$(PREFIX)),)
UNITDIR ?= $(LIBDIR)/systemd/user
else
UNITDIR ?= $(DATADIR)/systemd/user
endif

INSTALL         ?= install
INSTALL_PROGRAM ?= $(INSTALL) -Dm755
INSTALL_DATA    ?= $(INSTALL) -Dm644

REL_DIR := target/release
# Canal de instalación para el aviso de nueva versión (spec 0015): los paquetes
# nativos lo fijan a deb/rpm/arch; los repos de distro, a `distro`.
CHANNEL ?= source
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' crates/wayvpet/Cargo.toml | head -n1)
# RPM no admite '-' en la versión (sí '~'): 3.0.0-dev -> 3.0.0~dev.
RPMVER  := $(subst -,~,$(VERSION))
DIST    := dist

.PHONY: all build release debug test clippy fmt fmt-check install uninstall \
        dist deb rpm pkg checksums clean

all: release

build release:
	$(CARGO) build --release --workspace --locked

debug:
	$(CARGO) build --workspace

test:
	$(CARGO) test --workspace

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all --check

install: release
	$(INSTALL_PROGRAM) $(REL_DIR)/wayvpet            $(DESTDIR)$(BINDIR)/wayvpet
	$(INSTALL_PROGRAM) $(REL_DIR)/wayvpetctl         $(DESTDIR)$(BINDIR)/wayvpetctl
	$(INSTALL_PROGRAM) $(REL_DIR)/wayvpet-config     $(DESTDIR)$(BINDIR)/wayvpet-config
	$(INSTALL_PROGRAM) scripts/find_input_devices.sh $(DESTDIR)$(BINDIR)/wayvpet-find-devices
	$(INSTALL_DATA) wayvpet.conf.example $(DESTDIR)$(PKGDATADIR)/wayvpet.conf.example
	$(INSTALL_DATA) man/wayvpet.1        $(DESTDIR)$(MANDIR)/wayvpet.1
	@for f in presets/*.conf; do \
	  [ -f "$$f" ] || continue; \
	  echo "  presets/$$(basename $$f)"; \
	  $(INSTALL_DATA) "$$f" "$(DESTDIR)$(PKGDATADIR)/presets/$$(basename $$f)"; \
	done
	@find themes -type f | while read -r f; do \
	  $(INSTALL_DATA) "$$f" "$(DESTDIR)$(PKGDATADIR)/$$f"; \
	done
	$(INSTALL_DATA) packaging/wayvpet.desktop        $(DESTDIR)$(APPDIR)/wayvpet.desktop
	$(INSTALL_DATA) packaging/wayvpet-config.desktop $(DESTDIR)$(APPDIR)/wayvpet-config.desktop
	$(INSTALL_DATA) assets/tray/wayvpet-icon.png     $(DESTDIR)$(ICONDIR)/64x64/apps/wayvpet.png
	@for res in 16 24 32 48 64 128 256 512; do \
	  if [ -f "assets/icons/hicolor/$${res}x$${res}/apps/wayvpet.png" ]; then \
	    $(INSTALL) -d "$(DESTDIR)$(ICONDIR)/$${res}x$${res}/apps"; \
	    $(INSTALL_DATA) "assets/icons/hicolor/$${res}x$${res}/apps/wayvpet.png" "$(DESTDIR)$(ICONDIR)/$${res}x$${res}/apps/wayvpet.png"; \
	  fi; \
	done
	@$(INSTALL) -d $(DESTDIR)$(UNITDIR)
	sed 's|^ExecStart=wayvpet|ExecStart=$(BINDIR)/wayvpet|' packaging/systemd/wayvpet.service > $(DESTDIR)$(UNITDIR)/wayvpet.service
	@chmod 644 $(DESTDIR)$(UNITDIR)/wayvpet.service
	@$(INSTALL) -d "$(DESTDIR)$(PKGDATADIR)"
	@printf '%s\n' '$(CHANNEL)' > "$(DESTDIR)$(PKGDATADIR)/install-channel"
	@if [ -z "$(DESTDIR)" ]; then \
	  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$(DESTDIR)$(APPDIR)" >/dev/null 2>&1 || true; \
	  command -v gtk-update-icon-cache   >/dev/null 2>&1 && gtk-update-icon-cache -qtf "$(DESTDIR)$(ICONDIR)" >/dev/null 2>&1 || true; \
	fi
	@echo
	@echo "wayvpet $(VERSION) instalado en $(DESTDIR)$(PREFIX)."
	@echo "Autoarranque:  systemctl --user enable --now wayvpet.service"

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/wayvpet $(DESTDIR)$(BINDIR)/wayvpetctl \
	      $(DESTDIR)$(BINDIR)/wayvpet-config $(DESTDIR)$(BINDIR)/wayvpet-find-devices \
	      $(DESTDIR)$(MANDIR)/wayvpet.1 \
	      $(DESTDIR)$(APPDIR)/wayvpet.desktop $(DESTDIR)$(APPDIR)/wayvpet-config.desktop \
	      $(DESTDIR)$(ICONDIR)/*/apps/wayvpet.png \
	      $(DESTDIR)$(UNITDIR)/wayvpet.service
	rm -rf $(DESTDIR)$(PKGDATADIR)
	@if [ -z "$(DESTDIR)" ]; then \
	  if ls "$(APPDIR)"/*.desktop >/dev/null 2>&1; then \
	    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$(APPDIR)" >/dev/null 2>&1 || true; \
	  else rm -f "$(APPDIR)/mimeinfo.cache" 2>/dev/null; rmdir "$(APPDIR)" 2>/dev/null || true; fi; \
	  command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -qtf "$(ICONDIR)" >/dev/null 2>&1 || true; \
	fi
	@echo "wayvpet desinstalado. Tu configuración en ~/.config/wayvpet no se ha tocado."

# --- artefactos de release (spec 0002 M5) --------------------------------
# `make pkg` deja en dist/ el tarball + .deb + .rpm + SHA256SUMS. Los dos
# últimos necesitan herramientas extra:
#   cargo install cargo-deb cargo-generate-rpm

dist:
	@mkdir -p $(DIST)
	git archive --format=tar.gz --prefix=wayvpet-$(VERSION)/ -o $(DIST)/wayvpet-$(VERSION).tar.gz HEAD
	@echo "$(DIST)/wayvpet-$(VERSION).tar.gz"

deb:
	@command -v cargo-deb >/dev/null 2>&1 || { echo "falta cargo-deb:  cargo install cargo-deb"; exit 1; }
	@test -x $(REL_DIR)/wayvpet || { echo "corre 'make release' primero"; exit 1; }
	@mkdir -p $(DIST)
	$(CARGO) deb -p wayvpet --no-build --output $(DIST)
	@echo "$(DIST)/*.deb"

rpm:
	@command -v cargo-generate-rpm >/dev/null 2>&1 || { echo "falta cargo-generate-rpm:  cargo install cargo-generate-rpm"; exit 1; }
	@test -x $(REL_DIR)/wayvpet || { echo "corre 'make release' primero"; exit 1; }
	@mkdir -p $(DIST)
	$(CARGO) generate-rpm -p crates/wayvpet -s 'version = "$(RPMVER)"'
	@cp -v target/generate-rpm/*.rpm $(DIST)/
	@echo "$(DIST)/*.rpm"

checksums:
	@cd $(DIST) && sha256sum wayvpet* > SHA256SUMS && cat SHA256SUMS

pkg: release dist deb rpm checksums
	@echo; ls -1 $(DIST)

clean:
	$(CARGO) clean
	rm -rf $(DIST)
