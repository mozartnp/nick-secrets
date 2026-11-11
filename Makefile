# Runs every test suite of the repository (see tests/run_all.sh and ABOUT.md).
.DEFAULT_GOAL := test

.PHONY: test
.NOTPARALLEL:

test:
	@bash tests/run_all.sh
