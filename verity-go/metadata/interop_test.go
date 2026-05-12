// verity-go/metadata/interop_test.go
package metadata

import (
	"encoding/hex"
	"os"
	"path/filepath"
	"testing"

	"cryptpilot-verity-go/verity"
)

func interopDir() string {
	dir := os.Getenv("INTEROP_DIR")
	if dir == "" {
		dir = "/tmp/cryptpilot-interop"
	}
	return dir
}

// TestInterop_RustProducesGoVerifies reads metadata produced by the Rust
// `format` command and verifies it using Go's DeserializeMetadata + VerifySelf.
//
// Note: The metadata root hash (SHA-256 of FlatBuffers bytes) is NOT compared
// between Go and Rust because the FlatBuffers binary encoding differs across
// language implementations (vtable placement, byte alignment). Instead, we
// verify semantic equivalence: same file count, paths, labels, descriptor
// hashes, and per-file fs-verity calculations.
//
// Expected test data layout (created by `make interop-rust-produces`):
//
//	INTEROP_DIR/data/a.txt                            (content: "hello")
//	INTEROP_DIR/data/b.txt                            (content: "world")
//	INTEROP_DIR/data/empty.txt                        (content: "")
//	INTEROP_DIR/data/cryptpilot-verity.metadata.fb   (Rust format output)
//	INTEROP_DIR/root_hash.txt                         (root hash from Rust format)
//
// Set INTEROP_DIR env var to the directory, or defaults to /tmp/cryptpilot-interop.
func TestInterop_RustProducesGoVerifies(t *testing.T) {
	dir := interopDir()
	dataDir := filepath.Join(dir, "data")

	metadataPath := filepath.Join(dataDir, "cryptpilot-verity.metadata.fb")
	if _, err := os.Stat(metadataPath); os.IsNotExist(err) {
		t.Skip("interop data not found — run `make interop-rust-produces` first")
	}
	data, err := os.ReadFile(metadataPath)
	if err != nil {
		t.Fatalf("read metadata: %v", err)
	}

	info, err := DeserializeMetadata(data)
	if err != nil {
		t.Fatalf("deserialize: %v", err)
	}

	// Verify file count
	if len(info.FileInfos) != 3 {
		t.Fatalf("expected 3 files, got %d", len(info.FileInfos))
	}

	// Verify sorted paths
	expectedPaths := []string{"a.txt", "b.txt", "empty.txt"}
	for i, fi := range info.FileInfos {
		if fi.Path != expectedPaths[i] {
			t.Errorf("file[%d].path: expected %q, got %q", i, expectedPaths[i], fi.Path)
		}
	}

	// Verify labels
	if info.Labels["env"] != "prod" {
		t.Errorf("labels[env]: expected prod, got %q", info.Labels["env"])
	}

	// Verify each file's integrity (descriptor hash + root hash)
	for i, fi := range info.FileInfos {
		if err := fi.VerifySelf(); err != nil {
			t.Errorf("file[%d] (%s) verification failed: %v", i, fi.Path, err)
		}
	}

	// Re-read each data file and do block-level verify: recalculate fs-verity
	// and compare descriptor hash against what Rust stored.
	for _, fi := range info.FileInfos {
		filePath := filepath.Join(dataDir, fi.Path)
		content, err := os.ReadFile(filePath)
		if err != nil {
			t.Fatalf("read file %s: %v", fi.Path, err)
		}

		d := verity.NewFsVerity(verity.HashSHA256)
		d.Write(content)
		desc, tree := d.Finalize()
		recalcHash := hex.EncodeToString(desc.ToDescriptorHash())
		if recalcHash != fi.DescriptorHash {
			t.Errorf("block-level verify failed for %s: expected %s, got %s",
				fi.Path, fi.DescriptorHash, recalcHash)
		}

		rebuiltRoot := tree.RebuildRootHash(desc.Salt, desc.BlockSize())
		if !equalBytes(rebuiltRoot, desc.RootHash) {
			t.Errorf("root hash mismatch for %s", fi.Path)
		}
	}
}

// TestInterop_GoProducesRustVerifies creates test files and metadata,
// then writes them to INTEROP_DIR. The Makefile target `interop-go-produces`
// then runs `cargo run verify` to confirm Rust can consume Go's output.
func TestInterop_GoProducesRustVerifies(t *testing.T) {
	dir := interopDir()
	dataDir := filepath.Join(dir, "data")

	if err := os.MkdirAll(dataDir, 0755); err != nil {
		t.Fatalf("mkdir: %v", err)
	}

	testFiles := []struct {
		path    string
		content []byte
	}{
		{"hello.txt", []byte("hello")},
		{"world.txt", []byte("world")},
		{"empty.txt", []byte{}},
	}
	for _, tf := range testFiles {
		if err := os.WriteFile(filepath.Join(dataDir, tf.path), tf.content, 0644); err != nil {
			t.Fatalf("write file %s: %v", tf.path, err)
		}
	}

	// Build FileVerityInfo for each file
	fileInfos := make([]FileVerityInfo, len(testFiles))
	for i, tf := range testFiles {
		desc, tree := CalculateFsVerityHash(tf.content)
		fileInfos[i] = FileVerityInfo{
			Path:           tf.path,
			Descriptor:     desc,
			MerkleTree:     tree,
			DescriptorHash: hex.EncodeToString(desc.ToDescriptorHash()),
		}
	}

	labels := map[string]string{"env": "prod"}
	metadataBytes, err := SerializeMetadata(fileInfos, labels)
	if err != nil {
		t.Fatalf("serialize: %v", err)
	}

	metadataPath := filepath.Join(dataDir, "cryptpilot-verity.metadata.fb")
	if err := os.WriteFile(metadataPath, metadataBytes, 0644); err != nil {
		t.Fatalf("write metadata: %v", err)
	}

	t.Logf("Go produced metadata (%d bytes) for Rust verify", len(metadataBytes))
}
