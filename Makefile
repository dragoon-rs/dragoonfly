.PHONY: fmt fmt-check check clippy test show show-proxy unset-proxy toto

ssh_addr_file = 0

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
ifeq ($(ssh_addr_file),0)
	nu help_func/execute_all_tests.nu
else
	nu help_func/execute_all_tests.nu --ssh-addr-file $(ssh_addr_file)
endif

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

export-proxy:
	export HTTP_PROXY='proxy.isae.fr:3128'
