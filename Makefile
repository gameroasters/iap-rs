
check:
	cargo c
	cargo fmt -- --check
	cargo clean -p iap
	cargo clippy
	cargo t

clippy-nightly:
	cargo +nightly clean -p iap
	cargo +nightly clippy