.PHONY: fmt fmt-check check clippy test show show-proxy unset-proxy

DEFAULT_GOAL: fmt-check check clippy test

fmt-check:
	cargo fmt --all -- --check

fmt:
	cargo fmt --all

check:
	cargo check --workspace --profile ci-check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo build --release
	nu tests/help_func/execute_all_tests.nu

show:
	rustup --version
	rustup show --verbose
	rustc --version
	cargo --version
	cargo clippy --version
	nu --version

show-proxy:
	@echo "HTTP_PROXY = ${HTTP_PROXY}"
	@echo "http_proxy = ${http_proxy}" 
	@echo "HTTPS_PROXY = ${HTTPS_PROXY}"
	@echo "https_proxy = ${https_proxy}" 

unset-proxy:
	unset HTTP_PROXY
	unset http_proxy
	unset HTTPS_PROXY
	unset https_proxy

init:
	git submodule update --init komodo


export-proxy:
	export HTTP_PROXY='proxy.isae.fr:3128'