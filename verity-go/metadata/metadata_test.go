// verity-go/metadata/metadata_test.go
package metadata

import (
	"bytes"
	"encoding/hex"
	"testing"

	"cryptpilot-verity-go/verity"
)

func makeTestFileVerityInfo(path string, data []byte) FileVerityInfo {
	d := verity.NewFsVerity(verity.HashSHA256)
	d.Write(data)
	desc, tree := d.Finalize()
	return FileVerityInfo{
		Path:           path,
		Descriptor:     desc,
		MerkleTree:     tree,
		DescriptorHash: hex.EncodeToString(desc.ToDescriptorHash()),
	}
}

func TestSerializeDeserializeRoundTrip(t *testing.T) {
	info := makeTestFileVerityInfo("test.txt", []byte("test file content"))
	info.VerifySelf() // ensure test data is valid

	labels := map[string]string{"env": "prod"}
	serialized, err := SerializeMetadata([]FileVerityInfo{info}, labels)
	if err != nil {
		t.Fatalf("SerializeMetadata: %v", err)
	}

	deserialized, err := DeserializeMetadata(serialized)
	if err != nil {
		t.Fatalf("DeserializeMetadata: %v", err)
	}

	if len(deserialized.FileInfos) != 1 {
		t.Fatalf("expected 1 file, got %d", len(deserialized.FileInfos))
	}

	if deserialized.FileInfos[0].Path != "test.txt" {
		t.Errorf("path: expected test.txt, got %s", deserialized.FileInfos[0].Path)
	}
	if deserialized.FileInfos[0].DescriptorHash != info.DescriptorHash {
		t.Errorf("descriptor hash mismatch")
	}
	if deserialized.Labels["env"] != "prod" {
		t.Errorf("labels: expected env=prod, got %v", deserialized.Labels)
	}
}

func TestSerializeDeserializeEmptyLabels(t *testing.T) {
	info := makeTestFileVerityInfo("test.txt", []byte("test file content"))
	serialized, err := SerializeMetadata([]FileVerityInfo{info}, map[string]string{})
	if err != nil {
		t.Fatalf("SerializeMetadata: %v", err)
	}

	deserialized, err := DeserializeMetadata(serialized)
	if err != nil {
		t.Fatalf("DeserializeMetadata: %v", err)
	}

	if len(deserialized.Labels) != 0 {
		t.Errorf("expected empty labels, got %v", deserialized.Labels)
	}
}

func TestSerializeDeserializeMultipleFiles(t *testing.T) {
	infos := []FileVerityInfo{
		makeTestFileVerityInfo("b.txt", []byte("content b")),
		makeTestFileVerityInfo("a.txt", []byte("content a")),
		makeTestFileVerityInfo("c.txt", []byte("content c")),
	}
	labels := map[string]string{"key1": "val1", "key2": "val2"}

	serialized, err := SerializeMetadata(infos, labels)
	if err != nil {
		t.Fatalf("SerializeMetadata: %v", err)
	}

	deserialized, err := DeserializeMetadata(serialized)
	if err != nil {
		t.Fatalf("DeserializeMetadata: %v", err)
	}

	if len(deserialized.FileInfos) != 3 {
		t.Fatalf("expected 3 files, got %d", len(deserialized.FileInfos))
	}

	// Verify sorted order by path
	paths := []string{"a.txt", "b.txt", "c.txt"}
	for i, fi := range deserialized.FileInfos {
		if fi.Path != paths[i] {
			t.Errorf("file[%d]: expected path %s, got %s", i, paths[i], fi.Path)
		}
	}
}

func TestVerifySelf(t *testing.T) {
	info := makeTestFileVerityInfo("test.txt", []byte("test file content"))

	err := info.VerifySelf()
	if err != nil {
		t.Fatalf("VerifySelf: %v", err)
	}

	// Tamper with descriptor hash
	tampered := info
	tampered.DescriptorHash = "deadbeef"
	err = tampered.VerifySelf()
	if err == nil {
		t.Fatal("VerifySelf should fail with tampered descriptor hash")
	}
	if _, ok := err.(*VerificationError); !ok {
		t.Errorf("expected VerificationError, got %T", err)
	}
}

func TestVerifySelfRootHash(t *testing.T) {
	info := makeTestFileVerityInfo("test.txt", []byte("test file content"))

	// Tamper with root hash in descriptor
	tampered := info
	tampered.Descriptor.RootHash = bytes.Repeat([]byte{0xFF}, 32)
	err := tampered.VerifySelf()
	if err == nil {
		t.Fatal("VerifySelf should fail with tampered root hash")
	}
}

func TestMetadataVersionError(t *testing.T) {
	// The serialized data has version 1. If we manually tamper with it...
	// We can't easily tamper with FlatBuffers version field without low-level manipulation,
	// so just test the error type string representation.
	err := &MetadataVersionError{Version: 2}
	expected := "unsupported metadata version: 2, expected version 1"
	if err.Error() != expected {
		t.Errorf("error string: expected %q, got %q", expected, err.Error())
	}
}

func TestMetadataHashDeterminism(t *testing.T) {
	infos := []FileVerityInfo{
		makeTestFileVerityInfo("b.txt", []byte("content b")),
		makeTestFileVerityInfo("a.txt", []byte("content a")),
	}

	serialized, _ := SerializeMetadata(infos, map[string]string{"label": "value"})

	hash1, err := CalculateMetadataHash(serialized)
	if err != nil {
		t.Fatalf("CalculateMetadataHash: %v", err)
	}

	// Deserialize and re-serialize to verify hash is deterministic
	deserialized, _ := DeserializeMetadata(serialized)
	serialized2, _ := SerializeMetadata(deserialized.FileInfos, deserialized.Labels)

	hash2, err := CalculateMetadataHash(serialized2)
	if err != nil {
		t.Fatalf("CalculateMetadataHash (2): %v", err)
	}

	if hash1 != hash2 {
		t.Errorf("metadata hash not deterministic: first=%s second=%s", hash1, hash2)
	}
}

func TestMetadataHashEmpty(t *testing.T) {
	// Empty metadata: no files
	serialized, err := SerializeMetadata([]FileVerityInfo{}, map[string]string{})
	if err != nil {
		t.Fatalf("SerializeMetadata: %v", err)
	}

	hash, err := CalculateMetadataHash(serialized)
	if err != nil {
		t.Fatalf("CalculateMetadataHash: %v", err)
	}
	if len(hash) != 64 { // SHA-256 hex = 64 chars
		t.Errorf("hash length: expected 64, got %d", len(hash))
	}
}

func TestDeserializeError(t *testing.T) {
	_, err := DeserializeMetadata([]byte("not valid flatbuffers"))
	if err == nil {
		t.Fatal("expected error from invalid data")
	}
	if _, ok := err.(*ParseError); !ok {
		t.Errorf("expected ParseError, got %T", err)
	}
}

func TestCalculateFsVerityHash(t *testing.T) {
	desc, tree := CalculateFsVerityHash([]byte("test"))

	if desc.HashAlgorithm != verity.HashSHA256 {
		t.Errorf("expected SHA256 algorithm")
	}
	if tree == nil {
		t.Fatal("tree should not be nil")
	}
}
