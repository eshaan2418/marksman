SHELL := /bin/bash
CARGO ?= $(shell command -v cargo 2>/dev/null || echo $(HOME)/.cargo/bin/cargo)
J ?= $(shell n=$$(sysctl -n hw.ncpu 2>/dev/null || nproc); echo $$(( n / 2 > 2 ? n / 2 : 2 )))
export CARGO_BUILD_JOBS ?= $(J)

.PHONY: build test spec pathological bench fuzz lint demo clean

build:
	$(CARGO) build --release

spec:
	scripts/fetch_spec.sh

test: spec
	$(CARGO) test
	$(CARGO) test --release --test pathological

bench: spec
	scripts/make_corpus.sh
	$(CARGO) run --release -q --example conformance | tee bench/results/conformance.txt
	$(CARGO) bench --bench parse -- --noplot

FUZZTIME ?= 60
fuzz: spec
	$(CARGO) +nightly fuzz run to_html -- -max_total_time=$(FUZZTIME) -max_len=4096

lint:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings

demo: build
	@printf '# marksman\n\nA *CommonMark* parser: **652/652** spec examples.\n\n- lists\n- `code`\n\n> quotes and [links](https://commonmark.org "CommonMark")\n' | tee /dev/stderr | target/release/marksman

clean:
	$(CARGO) clean
