# verity-go

A Go implementation of Linux fs-verity, providing file integrity verification compatible with the kernel's fs-verity mechanism.

This is the Go counterpart of [cryptpilot-verity](https://github.com/openanolis/cryptpilot), sharing the same FlatBuffers schema via symlinks.

## Installation

```bash
go get github.com/openanolis/cryptpilot/verity-go
```

Or reference a specific commit:

```bash
go get github.com/openanolis/cryptpilot/verity-go@master
```

## Usage

### 1. Serialize directory metadata

The `metadata` package serializes a directory tree's integrity data: each file's
fs-verity descriptor, Merkle tree hashes, and a root hash for the entire directory.

#### Option A: Rust CLI (recommended)

The Rust `cryptpilot-verity` CLI handles the full pipeline — walk the directory,
compute fs-verity for each file, build the Merkle tree, and write the FlatBuffers
metadata file:

```bash
cargo run -p cryptpilot-verity -- format /path/to/dir --label env=prod -m metadata.fb --hash-output -
```

#### Option B: Go implementation

For programmatic control, compute per-file fs-verity and serialize in Go:

```go
package main

import (
	"encoding/hex"
	"os"

	"github.com/openanolis/cryptpilot/verity-go/metadata"
)

func main() {
	data, _ := os.ReadFile("somefile.txt")
	desc, tree := metadata.CalculateFsVerityHash(data)

	fileInfos := []metadata.FileVerityInfo{
		{
			Path:           "somefile.txt",
			Descriptor:     desc,
			MerkleTree:     tree,
			DescriptorHash: hex.EncodeToString(desc.ToDescriptorHash()),
		},
	}

	labels := map[string]string{"env": "prod"}
	fb, _ := metadata.SerializeMetadata(fileInfos, labels)
	os.WriteFile("metadata.fb", fb, 0644)
}
```

### 2. Deserialize and verify

Load the metadata bytes and run integrity checks. Three levels of verification
are available:

```go
package main

import (
	"fmt"
	"os"

	"github.com/openanolis/cryptpilot/verity-go/metadata"
)

func main() {
	data, _ := os.ReadFile("metadata.fb")
	info, _ := metadata.DeserializeMetadata(data)

	// (a) Metadata root hash — compare against a trusted reference value
	rootHash, _ := metadata.CalculateMetadataHash(data)
	expectedRootHash := "..." // trusted hash from build time or signing
	if rootHash != expectedRootHash {
		panic("metadata root hash mismatch!")
	}

	// (b) Self-verification — each file's descriptor hash and Merkle root
	for _, fi := range info.FileInfos {
		if err := fi.VerifySelf(); err != nil {
			fmt.Printf("%s: verification failed: %v\n", fi.Path, err)
		}
	}

	// (c) Per-block verification — verify a specific data block against the tree
	fi := info.FileInfos[0]
	blockSize := fi.Descriptor.BlockSize()
	fileData, _ := os.ReadFile(fi.Path)
	for blockIndex := 0; blockIndex*blockSize < len(fileData); blockIndex++ {
		start := blockIndex * blockSize
		end := start + blockSize
		if end > len(fileData) {
			end = len(fileData)
		}
		block := fileData[start:end]
		if !fi.MerkleTree.VerifyDataBlock(blockIndex, blockSize, block) {
			fmt.Printf("%s: block %d is corrupted!\n", fi.Path, blockIndex)
		}
	}
}
```

## Packages

| Package | Description |
|---------|-------------|
| `verity` | Core fs-verity: streaming hasher, Merkle tree, descriptor |
| `metadata` | FlatBuffers serialization/deserialization of directory integrity metadata |

## License

Same as the parent cryptpilot project. See the root `LICENSE` file.
