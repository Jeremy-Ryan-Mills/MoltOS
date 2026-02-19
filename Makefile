.PHONY: run build test

# Build the kernel
build:
	cargo build

# Run the kernel in QEMU using bootimage
run:
	bootimage run

# Run tests
test:
	cargo test

# Clean build artifacts
clean:
	cargo clean
	rm -f bootimage-*
