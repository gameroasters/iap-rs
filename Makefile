
check:
	cargo c
	cargo fmt -- --check
	cargo clean -p iap
	cargo clippy
	cargo t