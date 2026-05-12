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

### Compute fs-verity digest

```go
package main

import (
	"encoding/hex"
	"fmt"

	"github.com/openanolis/cryptpilot/verity-go/verity"
)

func main() {
	data := []byte("hello, world")

	d := verity.NewFsVerity(verity.HashSHA256)
	d.Write(data)
	desc, tree := d.Finalize()

	// The descriptor hash — the fs-verity "measurement" of this file
	fmt.Println(hex.EncodeToString(desc.ToDescriptorHash()))

	// Rebuild and verify the Merkle tree root hash
	root := tree.RebuildRootHash(desc.Salt, desc.BlockSize())
	fmt.Println(hex.EncodeToString(root))
}
```

### Serialize / deserialize directory metadata

The `metadata` package serializes a directory tree's integrity data: each file's
fs-verity descriptor, Merkle tree hashes, and a root hash for the entire directory.

```go
package main

import (
	"encoding/hex"
	"fmt"
	"os"

	"github.com/openanolis/cryptpilot/verity-go/metadata"
)

func main() {
	// Compute fs-verity for a file
	data, _ := os.ReadFile("somefile.txt")
	desc, tree := metadata.CalculateFsVerityHash(data)

	// Build metadata
	fileInfos := []metadata.FileVerityInfo{
		{
			Path:           "somefile.txt",
			Descriptor:     desc,
			MerkleTree:     tree,
			DescriptorHash: hex.EncodeToString(desc.ToDescriptorHash()),
		},
	}

	labels := map[string]string{"env": "prod"}

	// Serialize to FlatBuffers bytes
	fb, _ := metadata.SerializeMetadata(fileInfos, labels)
	os.WriteFile("metadata.fb", fb, 0644)

	// Deserialize and verify
	info, _ := metadata.DeserializeMetadata(fb)
	for _, fi := range info.FileInfos {
		if err := fi.VerifySelf(); err != nil {
			fmt.Printf("verification failed: %v\n", err)
		}
	}
}
```

## Packages

| Package | Description |
|---------|-------------|
| `verity` | Core fs-verity: streaming hasher, Merkle tree, descriptor |
| `metadata` | FlatBuffers serialization/deserialization of file verity metadata |

## License

Same as the parent cryptpilot project. See the root `LICENSE` file.
