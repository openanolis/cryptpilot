.PHONY: test clean-test build

# Test directory in /tmp
TEST_DIR := /tmp/cryptpilot-verity-test
TEST_DATA_DIR := $(TEST_DIR)/data
TEST_HASH_FILE := $(TEST_DIR)/root_hash.txt

# Build the project
build:
	cargo build

# Run tests: format, dump, and verify
test: clean-test
	@echo "=== Starting cryptpilot-verity test ==="
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
	
	@echo "=== All tests completed successfully ==="

# Clean test directory
clean-test:
	@echo "Cleaning test directory..."
	@rm -rf $(TEST_DIR)
	@echo "✓ Test directory cleaned"

