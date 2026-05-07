.PHONY: test clean-test build

# Test directory in /tmp
TEST_DIR := /tmp/cryptpilot-verity-test
TEST_DATA_DIR := $(TEST_DIR)/data
TEST_HASH_FILE := $(TEST_DIR)/root_hash.txt
# Bind mount point for accessing underlying data during tampering test
TEST_BIND_DIR := $(TEST_DIR)/data-bind

# Build the project
build:
	cargo build

# Run tests: format, dump, verify, open (in-place mount), and close
test: clean-test
	@echo "=== Starting cryptpilot-verity test (in-place mount scenario) ==="
	@echo ""
	
	@echo "Step 1: Creating test directory and files..."
	@mkdir -p $(TEST_DATA_DIR)
	@echo "test file 1 content" > $(TEST_DATA_DIR)/file1.txt
	@echo "test file 2 content" > $(TEST_DATA_DIR)/file2.txt
	@mkdir -p $(TEST_DATA_DIR)/subdir
	@echo "test file 3 in subdir" > $(TEST_DATA_DIR)/subdir/file3.txt
	@dd if=/dev/urandom of=$(TEST_DATA_DIR)/binary_file.bin bs=1024 count=10 2>/dev/null
	@echo "✓ Created test files in $(TEST_DATA_DIR)"
	@echo ""
	
	@echo "Step 2: Running format command..."
	@cargo run -- format $(TEST_DATA_DIR) --hash-output $(TEST_HASH_FILE)
	@echo "✓ Format completed, root hash saved to $(TEST_HASH_FILE)"
	@echo ""
	
	@echo "Step 3: Running dump command (print root hash)..."
	@cargo run -- dump $(TEST_DATA_DIR) --print-root-hash
	@echo ""
	
	@echo "Step 4: Running dump command (print metadata)..."
	@cargo run -- dump $(TEST_DATA_DIR) --print-metadata
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

	@echo "Step 8: Creating bind mount for tampering test later..."
	@echo "  → Creating bind mount to access underlying data during verity mount..."
	@mkdir -p $(TEST_BIND_DIR)
	@mount --bind $(TEST_DATA_DIR) $(TEST_BIND_DIR) 2>/dev/null || { echo "✗ Failed to create bind mount (may need sudo)"; exit 1; }
	@mount --make-private $(TEST_BIND_DIR) 2>/dev/null || true
	@echo "✓ Bind mount created at $(TEST_BIND_DIR) with private propagation"
	@echo ""


	
	@echo "Step 9: Testing open command (in-place mount with built-in FUSE)..."
	@HASH=$$(cat $(TEST_HASH_FILE)); \
	echo "  → Mounting verity-fuse in-place on $(TEST_DATA_DIR)..."; \
	cargo run -- open $(TEST_DATA_DIR) $(TEST_DATA_DIR) $$HASH 2>&1 & \
	MOUNT_PID=$$!; \
	sleep 3; \
	if mountpoint -q $(TEST_DATA_DIR) 2>/dev/null; then \
		echo "✓ Successfully mounted in-place at $(TEST_DATA_DIR)"; \
		echo "  → Listing mounted files:"; \
		ls -la $(TEST_DATA_DIR); \
		echo ""; \
		echo "  → Reading and verifying test file from mount:"; \
		FILE_CONTENT=$$(cat $(TEST_DATA_DIR)/file1.txt); \
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
		if dd if=$(TEST_DATA_DIR)/binary_file.bin of=/dev/null bs=1024 2>/dev/null; then \
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
	
	@echo "Step 10: Unmounting for tampering detection test..."
	@if mountpoint -q $(TEST_DATA_DIR) 2>/dev/null; then \
		echo "  → Unmounting in-place mount for tamper test..."; \
		fusermount -u $(TEST_DATA_DIR) 2>/dev/null || umount $(TEST_DATA_DIR) 2>/dev/null || true; \
		sleep 1; \
	fi
	@echo ""
	
	@echo "Step 11: Testing tampering detection (cache disabled, using bind mount)..."
	@echo "  → Tampering with underlying data via bind mount..."
	@echo "TAMPERED DATA" > $(TEST_BIND_DIR)/file1.txt
	@echo "  → Opening with --block-cache-capacity 0 to disable cache for tamper detection..."
	@HASH=$$(cat $(TEST_HASH_FILE)); \
	cargo run -- open $(TEST_DATA_DIR) $(TEST_DATA_DIR) $$HASH --block-cache-capacity 0 2>&1 & \
	MOUNT_PID=$$!; \
	sleep 3; \
	if mountpoint -q $(TEST_DATA_DIR) 2>/dev/null; then \
		echo "  → Attempting to read tampered file (should fail)..."; \
		if cat $(TEST_DATA_DIR)/file1.txt 2>/dev/null; then \
			echo "✗ ERROR: Tampered data was NOT detected!"; \
			echo "  This is a critical security failure."; \
			kill $$MOUNT_PID 2>/dev/null || true; \
			umount $(TEST_BIND_DIR) 2>/dev/null || true; \
			exit 1; \
		else \
			ERRCODE=$$?; \
			if [ $$ERRCODE -ne 0 ]; then \
				echo "✓ Tampering detected! Read failed with error code $$ERRCODE (expected)"; \
				echo "  Verification is working correctly."; \
			fi; \
		fi; \
	else \
		echo "✗ Remount for tampering test failed!"; \
		umount $(TEST_BIND_DIR) 2>/dev/null || true; \
		exit 1; \
	fi
	@echo ""
	@echo "  → Restoring original data via bind mount..."
	@echo "test file 1 content" > $(TEST_BIND_DIR)/file1.txt
	@echo "  → Remounting with cache enabled to verify restored file..."
	@fusermount -u $(TEST_DATA_DIR) 2>/dev/null || true
	@sleep 1
	@HASH=$$(cat $(TEST_HASH_FILE)); \
	cargo run -- open $(TEST_DATA_DIR) $(TEST_DATA_DIR) $$HASH 2>&1 & \
	MOUNT_PID=$$!; \
	sleep 3; \
	if cat $(TEST_DATA_DIR)/file1.txt >/dev/null 2>&1; then \
		echo "✓ Restored file reads successfully"; \
	else \
		echo "⚠ Restored file still fails (may need remount)"; \
	fi
	@echo "  → Cleaning up bind mount..."
	@umount $(TEST_BIND_DIR) 2>/dev/null || true
	@rmdir $(TEST_BIND_DIR) 2>/dev/null || true
	@echo ""
	
	@echo "Step 12: Unmounting after tampering test..."
	@if mountpoint -q $(TEST_DATA_DIR) 2>/dev/null; then \
		echo "  → Unmounting..."; \
		fusermount -u $(TEST_DATA_DIR) 2>/dev/null || cargo run -- close $(TEST_DATA_DIR) 2>/dev/null || true; \
		sleep 1; \
		if mountpoint -q $(TEST_DATA_DIR) 2>/dev/null; then \
			echo "⚠ Still mounted, force umounting..."; \
			umount $(TEST_DATA_DIR) 2>/dev/null || true; \
		else \
			echo "✓ Successfully unmounted $(TEST_DATA_DIR)"; \
		fi; \
	else \
		echo "⚠ Mount point was not active, skipping unmount"; \
	fi
	@echo ""
	
	@echo "Step 13: Testing open with custom block cache capacity..."
	@echo "  → Creating test data and formatting..."
	@mkdir -p $(TEST_DATA_DIR)/cache-test
	@echo "cache test content" > $(TEST_DATA_DIR)/cache-test/cache_file.txt
	@cargo run -- format $(TEST_DATA_DIR) --hash-output $(TEST_HASH_FILE) --force
	@echo "✓ Re-formatted with new test data"
	@echo ""
	@echo "  → Opening with --block-cache-capacity 1024 (4MB cache)..."
	@HASH=$$(cat $(TEST_HASH_FILE)); \
	cargo run -- open $(TEST_DATA_DIR) $(TEST_DATA_DIR) $$HASH --block-cache-capacity 1024 2>&1 & \
	MOUNT_PID=$$!; \
	sleep 3; \
	if mountpoint -q $(TEST_DATA_DIR) 2>/dev/null; then \
		echo "✓ Mounted with custom block cache capacity"; \
		echo "  → Reading file from capacity-limited mount:"; \
		FILE_CONTENT=$$(cat $(TEST_DATA_DIR)/cache-test/cache_file.txt); \
		if [ "$$FILE_CONTENT" = "cache test content" ]; then \
			echo "✓ File read correctly: $$FILE_CONTENT"; \
		else \
			echo "✗ File content mismatch!"; \
			echo "  Expected: cache test content"; \
			echo "  Got: $$FILE_CONTENT"; \
			kill $$MOUNT_PID 2>/dev/null || true; \
			exit 1; \
		fi; \
	else \
		echo "✗ Mount with custom block cache capacity failed!"; \
		exit 1; \
	fi
	@echo ""
	@echo "  → Unmounting block-cache-capacity test mount..."
	@fusermount -u $(TEST_DATA_DIR) 2>/dev/null || cargo run -- close $(TEST_DATA_DIR) 2>/dev/null || true
	@sleep 1
	@if mountpoint -q $(TEST_DATA_DIR) 2>/dev/null; then \
		echo "⚠ Still mounted, force umounting..."; \
		umount $(TEST_DATA_DIR) 2>/dev/null || true; \
	else \
		echo "✓ Unmounted successfully"; \
	fi
	@echo ""
	
	@echo "=== All tests completed successfully ==="

# Clean test directory
clean-test:
	@echo "Cleaning test directory..."
	@# Force unmount if still mounted
	@if mountpoint -q $(TEST_DATA_DIR) 2>/dev/null; then \
		echo "  → Unmounting $(TEST_DATA_DIR)..."; \
		fusermount -u $(TEST_DATA_DIR) 2>/dev/null || umount $(TEST_DATA_DIR) 2>/dev/null || true; \
	fi
	@# Clean up bind mount if exists
	@if mountpoint -q $(TEST_BIND_DIR) 2>/dev/null; then \
		echo "  → Unmounting bind mount $(TEST_BIND_DIR)..."; \
		umount $(TEST_BIND_DIR) 2>/dev/null || true; \
	fi
	@rm -rf $(TEST_DIR)
	@echo "✓ Test directory cleaned"

