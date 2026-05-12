// verity-go/verity/golden_test.go
package verity

import (
	"bytes"
	"encoding/hex"
	"os/exec"
	"path/filepath"
	"testing"
)

// expectedDescriptorHashes maps test file name to expected SHA-256 descriptor hash.
// These values come from verity-core/src/digest.rs tests (lines 538-551),
// which are the authoritative reference implementation.
var expectedDescriptorHashes = map[string]string{
	"empty":                 "3d248ca542a24fc62d1c43b916eae5016878e2533c88238480b26128a1f1af95",
	"onebyte":               "9845e616f7d2f7a1cd6742f0546a36d2e74d4eb8ae7d9bdc0b0df982c27861b7",
	"oneblock":              "3fd7a78101899a79cd337b1b4e5414be8bcb376b133370156ef6e65026d930ed",
	"oneblockplusonebyte":   "c0b9455d545b6b1ee5e7b227bd1ed463aaa530a4840dcd93465163a2b3aff0da",
	"hashblock_-1_0":        "c4b519068d8c8c68fd5e362fc3526c5b11e15f8eb72d4678017906f9e7f2d137",
	"hashblock_-1_-1":       "7aa0bb537c623562f898386ac88acd319267e4ab3200f3fd1cf648cfdb4a0379",
	"hashblock_-1_1":        "f804e9777f91d3697ca015303c23251ad3d80205184cfa3d1066ab28cb906330",
	"hashblock_0_0":         "f5c2b9ded1595acfe8a996795264d488dd6140531f6a01f8f8086a83fd835935",
	"hashblock_0_-1":        "5c00a54bd1d8341d7bbad060ff1b8e88ed2646d7bb38db6e752cd1cff66c0a78",
	"hashblock_0_1":         "a7abb76568871169a79104d00679fae6521dfdb2a2648e380c02b10e96e217ff",
	"hashblock_1_0":         "09510d2dbb55fa16f2768165c42d19c4da43301dfaa05705b2ecb4aaa4a5686a",
	"hashblock_1_-1":        "26159b4fc68c63881c25c33b23f2583ffaa64fee411af33c3b03238eea56755c",
	"hashblock_1_1":         "57bed0934bf3ab4610d54938f03cff27bd0d9d76c9a77e283f9fb2b7e29c5ab8",
}

func TestVerityAgainstRustReference(t *testing.T) {
	// Find repo root (verity-go is a subdirectory of the repo root)
	repoRoot, err := findRepoRoot()
	if err != nil {
		t.Skipf("cannot find repo root: %v", err)
	}

	// Run make_testfiles.py to generate testfiles/
	script := filepath.Join(repoRoot, "verity-core", "make_testfiles.py")
	cmd := exec.Command("python3", script)
	cmd.Dir = repoRoot
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("make_testfiles.py failed: %v\n%s", err, out)
	}

	testfilesDir := filepath.Join(repoRoot, "verity-core", "testfiles")

	for name, expectedHash := range expectedDescriptorHashes {
		t.Run(name, func(t *testing.T) {
			data, err := readFile(filepath.Join(testfilesDir, name))
			if err != nil {
				t.Fatalf("read testfile: %v", err)
			}

			d := NewFsVerity(HashSHA256)
			d.Write(data)
			desc, tree := d.Finalize()

			actualHash := hex.EncodeToString(desc.ToDescriptorHash())
			if actualHash != expectedHash {
				t.Errorf("descriptor hash mismatch\nexpected: %s\ngot:      %s", expectedHash, actualHash)
			}

			rebuiltRoot := tree.RebuildRootHash(desc.Salt, desc.BlockSize())
			if !bytes.Equal(rebuiltRoot, desc.RootHash) {
				t.Errorf("root hash mismatch after rebuild")
			}
		})
	}
}

func findRepoRoot() (string, error) {
	out, err := exec.Command("git", "rev-parse", "--show-toplevel").CombinedOutput()
	if err != nil {
		return "", err
	}
	return string(bytes.TrimSpace(out)), nil
}

func readFile(path string) ([]byte, error) {
	return exec.Command("cat", path).Output()
}
