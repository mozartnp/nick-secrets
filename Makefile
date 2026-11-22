# Runs every test suite of the repository (see tests/run_all.sh and ABOUT.md), and installs or
# uninstalls the nick in PREFIX (see install/install.sh and INSTALL.md). `make` alone runs the
# tests: it never installs anything.
.DEFAULT_GOAL := test
PREFIX ?= $(HOME)/.local

.PHONY: test
.PHONY: install uninstall
.NOTPARALLEL:

test:
	@bash tests/run_all.sh

install:
	@bash install/install.sh install --prefix "$(PREFIX)"

uninstall:
	@bash install/install.sh uninstall --prefix "$(PREFIX)"
