# verity-go

Linux fs-verity 的 Go 实现，提供与内核 fs-verity 机制兼容的文件完整性校验。

这是 [cryptpilot-verity](https://github.com/openanolis/cryptpilot) Rust 实现的 Go 对应版本，通过 symlink 共享相同的 FlatBuffers schema。

## 安装

```bash
go get github.com/openanolis/cryptpilot/verity-go
```

或者引用特定提交：

```bash
go get github.com/openanolis/cryptpilot/verity-go@master
```

## 使用方法

### 计算 fs-verity 摘要

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

	// 文件的 fs-verity "度量值"（descriptor hash）
	fmt.Println(hex.EncodeToString(desc.ToDescriptorHash()))

	// 重建并验证 Merkle tree root hash
	root := tree.RebuildRootHash(desc.Salt, desc.BlockSize())
	fmt.Println(hex.EncodeToString(root))
}
```

### 序列化 / 反序列化元数据

```go
package main

import (
	"encoding/hex"
	"fmt"
	"os"

	"github.com/openanolis/cryptpilot/verity-go/metadata"
)

func main() {
	// 计算文件的 fs-verity
	data, _ := os.ReadFile("somefile.txt")
	desc, tree := metadata.CalculateFsVerityHash(data)

	// 构建元数据
	fileInfos := []metadata.FileVerityInfo{
		{
			Path:           "somefile.txt",
			Descriptor:     desc,
			MerkleTree:     tree,
			DescriptorHash: hex.EncodeToString(desc.ToDescriptorHash()),
		},
	}

	labels := map[string]string{"env": "prod"}

	// 序列化为 FlatBuffers 字节
	fb, _ := metadata.SerializeMetadata(fileInfos, labels)
	os.WriteFile("metadata.fb", fb, 0644)

	// 反序列化并验证
	info, _ := metadata.DeserializeMetadata(fb)
	for _, fi := range info.FileInfos {
		if err := fi.VerifySelf(); err != nil {
			fmt.Printf("验证失败: %v\n", err)
		}
	}
}
```

## 包说明

| 包 | 说明 |
|----|------|
| `verity` | 核心 fs-verity：流式 hasher、Merkle tree、descriptor |
| `metadata` | FlatBuffers 序列化/反序列化文件 verity 元数据 |

## 许可证

与父项目 cryptpilot 相同。见根目录下的 `LICENSE` 文件。
