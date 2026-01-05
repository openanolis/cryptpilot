
NAME = verity-fuse
TARGET_DIR = ../target/release
BINARY = $(TARGET_DIR)/$(NAME)

# Use /tmp/test-data as default source directory
SOURCE_DIR = /tmp/test-data
MOUNT_POINT = /tmp/fuse-mount

.PHONY: build run mount unmount clean fmt lint logs setup_test_data

# Build the project
build:
	cargo build --release

# Setup test data: create test directory with rich content
setup_test_data: clean_test_data
	@echo "Creating test data in $(SOURCE_DIR)..."
	@mkdir -p "$(SOURCE_DIR)"

	# === 1. Root directory files ===
	echo "Hello, this is a regular text file." > "$(SOURCE_DIR)/hello.txt"
	echo "" > "$(SOURCE_DIR)/empty.txt"                    # Empty file
	printf "\x00\x01\x02\xFF\xFE" > "$(SOURCE_DIR)/binary.dat"  # Binary file
	echo "readonly content" > "$(SOURCE_DIR)/readonly.txt" 
	chmod 400 "$(SOURCE_DIR)/readonly.txt"                 # Read-only file
	touch "$(SOURCE_DIR)/executable.sh" && \
		echo '#!/bin/bash\necho "I am executable"' > "$(SOURCE_DIR)/executable.sh" && \
		chmod +x "$(SOURCE_DIR)/executable.sh"

	# === 2. Hidden files ===
	echo "This is a hidden config file." > "$(SOURCE_DIR)/.config.ini"
	echo "Secret backup" > "$(SOURCE_DIR)/.backup.tar.gz"

	# === 3. Special character filenames ===
	echo "File with space" > "$(SOURCE_DIR)/my document.pdf"
	echo "中文文件名测试" > "$(SOURCE_DIR)/文档说明.txt"
	echo "Symbolic link test" > "$(SOURCE_DIR)/target_file"

	# === 4. Multi-level directory structure ===
	mkdir -p "$(SOURCE_DIR)/docs"
	mkdir -p "$(SOURCE_DIR)/logs"
	mkdir -p "$(SOURCE_DIR)/assets/images"
	mkdir -p "$(SOURCE_DIR)/project/src"
	mkdir -p "$(SOURCE_DIR)/backups/yearly"

	# Populate docs
	echo "# Project Documentation\n\n- Version: 1.0\n- Author: Tester" > "$(SOURCE_DIR)/docs/README.md"
	echo "Generated log entry at $(date)" > "$(SOURCE_DIR)/logs/app.log"
	echo "More logs..." >> "$(SOURCE_DIR)/logs/app.log"

	# Large file test (1MB)
	dd if=/dev/zero of="$(SOURCE_DIR)/assets/images/large.jpg" bs=1K count=1024 status=none
	# Small file
	echo "icon" > "$(SOURCE_DIR)/assets/images/icon.png"

	# Project files
	echo 'fn main() { println!("Hello from Rust!"); }' > "$(SOURCE_DIR)/project/src/main.rs"
	echo 'console.log("Hello");' > "$(SOURCE_DIR)/project/src/index.js"

	# Backups
	date > "$(SOURCE_DIR)/backups/yearly/backup-$(shell date +%Y-%m-%d).txt"

	# === 5. Symbolic links ===
	ln -sf "../hello.txt" "$(SOURCE_DIR)/link_to_hello.txt"
	ln -sf "images/large.jpg" "$(SOURCE_DIR)/assets/screenshot.jpg"
	ln -sf ".config.ini" "$(SOURCE_DIR)/config-link"

	# === 6. Permission complexity (simulation) ===
	chmod 600 "$(SOURCE_DIR)/private-secret" 2>/dev/null || true

	@echo "✅ Test data created at $(SOURCE_DIR)"
	@echo "📁 Directory tree:"
	@find "$(SOURCE_DIR)" -type f | sort | sed 's|[^/]*/|- |g'

# Quick run (build + setup data + start)
run: build setup_test_data
	mkdir -p "$(MOUNT_POINT)"
	./$(BINARY) --source "$(SOURCE_DIR)" --mount-point "$(MOUNT_POINT)"

# Mount in background
mount: build setup_test_data
	@echo "Unmounting previous instance (if any)..."
	-fusermount -u "$(MOUNT_POINT)"
	@echo "Mounting FUSE filesystem on $(MOUNT_POINT)..."
	./$(BINARY) --source "$(SOURCE_DIR)" --mount-point "$(MOUNT_POINT)" &

	sleep 2
	@if mountpoint -q "$(MOUNT_POINT)"; then \
		echo "✅ Successfully mounted on $(MOUNT_POINT)"; \
	else \
		echo "❌ Mount failed or not visible."; \
	fi

# Unmount and cleanup
unmount:
	@echo "Unmounting $(MOUNT_POINT)..."
	-fusermount -u "$(MOUNT_POINT)"
	-pkill -f $(NAME) || true
	@echo "Unmounted and killed background process."

clean_test_data:
	@echo "Removing $(SOURCE_DIR) and $(MOUNT_POINT)..."
	-rm -rf "$(SOURCE_DIR)"
	-rm -rf "$(MOUNT_POINT)"

clean: clean_test_data
	cargo clean

fmt:
	cargo fmt

lint:
	cargo clippy --all-targets -- -D warnings

logs:
	@echo "💡 Logs are printed to stdout/stderr."
	@echo "To monitor system logs (if using journal):"
	@echo "   journalctl -f | grep -i fuse"

status:
	@if mountpoint -q "$(MOUNT_POINT)"; then \
		echo "🟢 $(MOUNT_POINT) is mounted"; \
		ls -la "$(MOUNT_POINT)"; \
	else \
		echo "🔴 $(MOUNT_POINT) is not mounted"; \
	fi

tree:
	@echo "📁 Current source directory structure:"
	find "$(SOURCE_DIR)" -print

help:
	@echo ""
	@echo "🚀 Passthrough FUSE Filesystem - Development Makefile"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Main targets:"
	@echo "  setup_test_data    - Create rich test data in $(SOURCE_DIR)"
	@echo "  build              - Compile release binary"
	@echo "  run                - Build + setup + run foreground"
	@echo "  mount              - Build + setup + run in background"
	@echo "  unmount            - Unmount and kill process"
	@echo "  status             - Check if mounted"
	@echo "  tree               - Show source directory tree"
	@echo "  logs               - Tips for viewing logs"
	@echo "  clean              - Remove binaries and test dirs"
	@echo "  fmt                - Format code with rustfmt"
	@echo "  lint               - Run clippy linting"
	@echo ""
	@echo "💡 Example workflow:"
	@echo "   make mount        # Start service"
	@echo "   ls $(MOUNT_POINT)"
	@echo "   cat $(MOUNT_POINT)/hello.txt"
	@echo "   make unmount      # Stop"
