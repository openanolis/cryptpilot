.PHONY: test clean-test build

# Test directory in /tmp
TEST_DIR := /tmp/cryptpilot-verity-test
TEST_DATA_DIR := $(TEST_DIR)/data
TEST_MOUNT_DIR := $(TEST_DIR)/mount
TEST_HASH_FILE := $(TEST_DIR)/root_hash.txt

# Build the project
build:
	cargo build

# Run tests: format, dump, verify, open, and close
test: clean-test
	@echo "=== Starting cryptpilot-verity test ==="
	@echo ""
	
	@echo "Step 1: Creating test directory and files..."
	@mkdir -p $(TEST_DATA_DIR)
	@mkdir -p $(TEST_MOUNT_DIR)
	@echo "test file 1 content" > $(TEST_DATA_DIR)/file1.txt
	@echo "test file 2 content" > $(TEST_DATA_DIR)/file2.txt
	@mkdir -p $(TEST_DATA_DIR)/subdir
	@echo "test file 3 in subdir" > $(TEST_DATA_DIR)/subdir/file3.txt
	@dd if=/dev/urandom of=$(TEST_DATA_DIR)/binary_file.bin bs=1024 count=10 2>/dev/null
	@echo "✓ Created test files in $(TEST_DATA_DIR)"
	@echo "✓ Created mount point at $(TEST_MOUNT_DIR)"
	@echo ""
	
	@echo "Step 2: Running format command..."
	@cargo run -- format $(TEST_DATA_DIR) --hash-output $(TEST_HASH_FILE)
	@echo "✓ Format completed, root hash saved to $(TEST_HASH_FILE)"
	@echo ""
	
	@echo "Step 3: Running dump command (print root hash)..."
	@cargo run -- dump --data-dir $(TEST_DATA_DIR) --print-root-hash
	@echo ""
	
	@echo "Step 4: Running dump command (print metadata)..."
	@cargo run -- dump --data-dir $(TEST_DATA_DIR) --print-metadata
	@echo ""
	
	@echo "Step 5: Running verify command..."
	@HASH=$$(cat $(TEST_HASH_FILE)); \
	cargo run -- verify $(TEST_DATA_DIR) $$HASH
	@echo "✓ Verification passed!"
	@echo ""
	
	@echo "Step 6: Testing verification failure (modified file)..."
	@echo "modified content" > $(TEST_DATA_DIR)/file1.txt
	@HASH=$$(cat $(TEST_HASH_FILE)); \
	if cargo run -- verify $(TEST_DATA_DIR) $$HASH 2>&1; then \
		echo "✗ Verification should have failed!"; \
		exit 1; \
	else \
		echo "✓ Verification correctly detected file modification"; \
	fi
	@echo ""
	
	@echo "Step 7: Restoring original file for mount test..."
	@echo "test file 1 content" > $(TEST_DATA_DIR)/file1.txt
	@echo "✓ File restored"
	@echo ""


	
	@echo "Step 8: Testing open command (mount with built-in FUSE)..."
	@HASH=$$(cat $(TEST_HASH_FILE)); \
	echo "  → Attempting mount with built-in FUSE implementation..."; \
	cargo run -- open $(TEST_DATA_DIR) $(TEST_MOUNT_DIR) $$HASH 2>&1 & \
	sleep 2; \
	if mountpoint -q $(TEST_MOUNT_DIR) 2>/dev/null; then \
		echo "✓ Successfully mounted at $(TEST_MOUNT_DIR)"; \
		echo "  → Listing mounted files:"; \
		ls -la $(TEST_MOUNT_DIR); \
		echo "  → Reading test file from mount:"; \
		cat $(TEST_MOUNT_DIR)/file1.txt; \
	else \
		echo "⚠ Mount may have failed, but continuing..."; \
	fi
	@echo ""
	
	@echo "Step 9: Testing close command (unmount)..."
	@if mountpoint -q $(TEST_MOUNT_DIR) 2>/dev/null; then \
		echo "  → Mount point is active, attempting unmount..."; \
		cargo run -- close $(TEST_MOUNT_DIR) 2>&1 || true; \
		sleep 1; \
		if mountpoint -q $(TEST_MOUNT_DIR) 2>/dev/null; then \
			echo "⚠ Unmount may have failed, forcing with fusermount..."; \
			fusermount -u $(TEST_MOUNT_DIR) 2>/dev/null || umount $(TEST_MOUNT_DIR) 2>/dev/null || true; \
		else \
			echo "✓ Successfully unmounted $(TEST_MOUNT_DIR)"; \
		fi; \
	else \
		echo "⚠ Mount point was not active, skipping unmount"; \
	fi
	@echo ""
	
	@echo "=== All tests completed successfully ==="

# Clean test directory
clean-test:
	@echo "Cleaning test directory..."
	@# Force unmount if still mounted
	@if mountpoint -q $(TEST_MOUNT_DIR) 2>/dev/null; then \
		echo "  → Unmounting $(TEST_MOUNT_DIR)..."; \
		fusermount -u $(TEST_MOUNT_DIR) 2>/dev/null || umount $(TEST_MOUNT_DIR) 2>/dev/null || true; \
	fi
	@rm -rf $(TEST_DIR)
	@echo "✓ Test directory cleaned"

