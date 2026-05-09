// verity-go/verity/tree.go
package verity

// MerkleTree stores level-1 hashes (one per data block).
type MerkleTree struct {
	level1 [][]byte
	algo   HashAlgorithm
}

// NewMerkleTree creates a tree from level-1 hashes.
func NewMerkleTree(hashes [][]byte, algo HashAlgorithm) *MerkleTree {
	return &MerkleTree{level1: hashes, algo: algo}
}

// Level1AsBytes returns the level-1 hashes as concatenated bytes.
func (t *MerkleTree) Level1AsBytes() []byte {
	digestSize := t.algo.digestSize()
	out := make([]byte, 0, len(t.level1)*digestSize)
	for _, h := range t.level1 {
		out = append(out, h...)
	}
	return out
}

// RebuildRootHash reconstructs the root hash from level-1 hashes.
// Used when loading a tree from serialized metadata.
// When there are more level-1 hashes than fit in one block, this builds
// intermediate tree levels using the full FsVerityDigest pipeline.
func (t *MerkleTree) RebuildRootHash(salt []byte, blockSize int) []byte {
	if len(t.level1) == 0 {
		// Empty file: root hash is all zeros
		return make([]byte, t.algo.digestSize())
	}
	if len(t.level1) == 1 {
		return t.level1[0]
	}

	// Use a full FsVerityDigest to process the level-1 hashes,
	// matching the Rust implementation's rebuild_root_hash.
	d := NewFsVerityWithSaltAndBlockSize(t.algo, salt, blockSize)
	for _, hash := range t.level1 {
		d.Write(hash)
	}
	// Pad to block boundary
	totalBytes := len(t.level1) * t.algo.digestSize()
	padding := (blockSize - totalBytes%blockSize) % blockSize
	if padding > 0 {
		d.Write(make([]byte, padding))
	}
	desc, _ := d.Finalize()
	return desc.RootHash
}

// VerifyDataBlock verifies a single data block against the merkle tree.
func (t *MerkleTree) VerifyDataBlock(blockIndex int, blockSize int, data []byte) bool {
	if len(data) > blockSize {
		return false
	}
	if blockIndex >= len(t.level1) {
		return false
	}
	expected := t.level1[blockIndex]

	h := t.algo.newHash()
	h.Write(data)
	// Pad to block size
	if len(data) < blockSize {
		h.Write(make([]byte, blockSize-len(data)))
	}
	actual := h.Sum(nil)

	return equalBytes(actual, expected)
}

func equalBytes(a, b []byte) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}
