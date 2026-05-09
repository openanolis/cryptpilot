// verity-go/verity/verity_test.go
package verity

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"testing"
)

func TestEmptyFile(t *testing.T) {
	d := NewFsVerity(HashSHA256)
	desc, tree := d.Finalize()

	// sha256:3d248ca542a24fc62d1c43b916eae5016878e2533c88238480b26128a1f1af95
	expected := "3d248ca542a24fc62d1c43b916eae5016878e2533c88238480b26128a1f1af95"
	actual := hex.EncodeToString(desc.ToDescriptorHash())
	if actual != expected {
		t.Errorf("empty: expected %s, got %s", expected, actual)
	}

	// Root hash should be all zeros for empty file
	expectedRoot := make([]byte, 32)
	if !equalBytes(desc.RootHash, expectedRoot) {
		t.Errorf("empty: root hash should be all zeros, got %s", hex.EncodeToString(desc.RootHash))
	}

	rebuiltRoot := tree.RebuildRootHash(desc.Salt, desc.BlockSize())
	if !equalBytes(rebuiltRoot, desc.RootHash) {
		t.Errorf("empty: root hash mismatch: desc=%s rebuilt=%s",
			hex.EncodeToString(desc.RootHash), hex.EncodeToString(rebuiltRoot))
	}
}

func TestOneByte(t *testing.T) {
	d := NewFsVerity(HashSHA256)
	d.Write([]byte{'A'}) // matches Python: b'A'
	desc, tree := d.Finalize()

	expected := "9845e616f7d2f7a1cd6742f0546a36d2e74d4eb8ae7d9bdc0b0df982c27861b7"
	actual := hex.EncodeToString(desc.ToDescriptorHash())
	if actual != expected {
		t.Errorf("onebyte: expected %s, got %s", expected, actual)
	}

	// Verify root hash
	rebuiltRoot := tree.RebuildRootHash(desc.Salt, desc.BlockSize())
	if !equalBytes(rebuiltRoot, desc.RootHash) {
		t.Errorf("onebyte: root hash mismatch")
	}

	// Verify data block
	block := make([]byte, desc.BlockSize())
	block[0] = 'A'
	if !tree.VerifyDataBlock(0, desc.BlockSize(), block[:1]) {
		t.Errorf("onebyte: data block verification failed")
	}
}

func TestOneBlock(t *testing.T) {
	// Exactly 4096 bytes of 'A'
	data := make([]byte, DefaultBlockSize)
	for i := range data {
		data[i] = 'A'
	}
	d := NewFsVerity(HashSHA256)
	d.Write(data)
	desc, tree := d.Finalize()

	expected := "3fd7a78101899a79cd337b1b4e5414be8bcb376b133370156ef6e65026d930ed"
	actual := hex.EncodeToString(desc.ToDescriptorHash())
	if actual != expected {
		t.Errorf("oneblock: expected %s, got %s", expected, actual)
	}

	rebuiltRoot := tree.RebuildRootHash(desc.Salt, desc.BlockSize())
	if !equalBytes(rebuiltRoot, desc.RootHash) {
		t.Errorf("oneblock: root hash mismatch")
	}
}

func TestOneBlockPlusOneByte(t *testing.T) {
	// 4096 bytes of 'A' + 1 byte 'B'
	data := make([]byte, DefaultBlockSize+1)
	for i := 0; i < DefaultBlockSize; i++ {
		data[i] = 'A'
	}
	data[DefaultBlockSize] = 'B'
	d := NewFsVerity(HashSHA256)
	d.Write(data)
	desc, tree := d.Finalize()

	expected := "c0b9455d545b6b1ee5e7b227bd1ed463aaa530a4840dcd93465163a2b3aff0da"
	actual := hex.EncodeToString(desc.ToDescriptorHash())
	if actual != expected {
		t.Errorf("oneblockplusonebyte: expected %s, got %s", expected, actual)
	}

	rebuiltRoot := tree.RebuildRootHash(desc.Salt, desc.BlockSize())
	if !equalBytes(rebuiltRoot, desc.RootHash) {
		t.Errorf("oneblockplusonebyte: root hash mismatch")
	}
}

// hashblock test cases: block_size * (hashes_per_block + i) + j bytes of 'A'
// hashes_per_block = 4096 / 32 = 128
func TestHashblock(t *testing.T) {
	hashesPerBlock := DefaultBlockSize / sha256.Size // 128
	tests := []struct {
		i, j     int
		expected string
	}{
		{0, 0, "f5c2b9ded1595acfe8a996795264d488dd6140531f6a01f8f8086a83fd835935"},
		{0, -1, "5c00a54bd1d8341d7bbad060ff1b8e88ed2646d7bb38db6e752cd1cff66c0a78"},
		{0, 1, "a7abb76568871169a79104d00679fae6521dfdb2a2648e380c02b10e96e217ff"},
		{-1, 0, "c4b519068d8c8c68fd5e362fc3526c5b11e15f8eb72d4678017906f9e7f2d137"},
		{1, 0, "09510d2dbb55fa16f2768165c42d19c4da43301dfaa05705b2ecb4aaa4a5686a"},
		{-1, -1, "7aa0bb537c623562f898386ac88acd319267e4ab3200f3fd1cf648cfdb4a0379"},
		{-1, 1, "f804e9777f91d3697ca015303c23251ad3d80205184cfa3d1066ab28cb906330"},
		{1, -1, "26159b4fc68c63881c25c33b23f2583ffaa64fee411af33c3b03238eea56755c"},
		{1, 1, "57bed0934bf3ab4610d54938f03cff27bd0d9d76c9a77e283f9fb2b7e29c5ab8"},
	}
	for _, tc := range tests {
		name := fmt.Sprintf("hashblock_%d_%d", tc.i, tc.j)
		size := DefaultBlockSize*(hashesPerBlock+tc.i) + tc.j
		if size < 0 {
			size = 0
		}
		data := bytes.Repeat([]byte{'A'}, size)
		d := NewFsVerity(HashSHA256)
		d.Write(data)
		desc, tree := d.Finalize()

		actual := hex.EncodeToString(desc.ToDescriptorHash())
		if actual != tc.expected {
			t.Errorf("%s: expected %s, got %s", name, tc.expected, actual)
		}

		rebuiltRoot := tree.RebuildRootHash(desc.Salt, desc.BlockSize())
		if !equalBytes(rebuiltRoot, desc.RootHash) {
			t.Errorf("%s: root hash mismatch", name)
		}
	}
}
