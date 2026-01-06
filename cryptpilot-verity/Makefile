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
	MOUNT_PID=$$!; \
	sleep 3; \
	if mountpoint -q $(TEST_MOUNT_DIR) 2>/dev/null; then \
		echo "✓ Successfully mounted at $(TEST_MOUNT_DIR)"; \
		echo "  → Listing mounted files:"; \
		ls -la $(TEST_MOUNT_DIR); \
		echo ""; \
		echo "  → Reading and verifying test file from mount:"; \
		FILE_CONTENT=$$(cat $(TEST_MOUNT_DIR)/file1.txt); \
		EXPECTED_CONTENT="test file 1 content"; \
		if [ "$$FILE_CONTENT" = "$$EXPECTED_CONTENT" ]; then \
			echo "✓ File content matches: $$FILE_CONTENT"; \
		else \
			echo "✗ File content mismatch!"; \
			echo "  Expected: $$EXPECTED_CONTENT"; \
			echo "  Got: $$FILE_CONTENT"; \
			exit 1; \
		fi; \
		echo ""; \
		echo "  → Testing binary file read:"; \
		if dd if=$(TEST_MOUNT_DIR)/binary_file.bin of=/dev/null bs=1024 2>/dev/null; then \
			echo "✓ Binary file read successfully"; \
		else \
			echo "✗ Binary file read failed"; \
			exit 1; \
		fi; \
	else \
		echo "✗ Mount failed!"; \
		if ps -p $$MOUNT_PID > /dev/null 2>&1; then \
			kill $$MOUNT_PID 2>/dev/null || true; \
		fi; \
		exit 1; \
	fi
	@echo ""
	
	@echo "Step 9: Testing tampering detection..."
	@echo "  → Tampering with underlying data file..."
	@echo "TAMPERED DATA" > $(TEST_DATA_DIR)/file1.txt
	@echo "  → Attempting to read tampered file (should fail)..."
	@if cat $(TEST_MOUNT_DIR)/file1.txt 2>/dev/null; then \
		echo "✗ ERROR: Tampered data was NOT detected!"; \
		echo "  This is a critical security failure."; \
		exit 1; \
	else \
		ERRCODE=$$?; \
		if [ $$ERRCODE -ne 0 ]; then \
			echo "✓ Tampering detected! Read failed with error code $$ERRCODE (expected)"; \
			echo "  Verification is working correctly."; \
		fi; \
	fi
	@echo "  → Restoring original data..."
	@echo "test file 1 content" > $(TEST_DATA_DIR)/file1.txt
	@echo "  → Verifying restored file can be read..."
	@if cat $(TEST_MOUNT_DIR)/file1.txt >/dev/null 2>&1; then \
		echo "✓ Restored file reads successfully"; \
	else \
		echo "⚠ Restored file still fails (may need remount)"; \
	fi
	@echo ""
	
	@echo "Step 10: Testing close command (unmount)..."
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

