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

### 1. 序列化目录元数据

`metadata` 包用于将整个目录树的完整性数据序列化：包含每个文件的
fs-verity 描述符、Merkle 树哈希，以及整个目录的根哈希。

#### 方式 A：Rust CLI（推荐）

Rust `cryptpilot-verity` CLI 可自动完成完整流程 — 遍历目录、
计算每个文件的 fs-verity、构建 Merkle 树、输出 FlatBuffers 元数据文件：

```bash
cargo run -p cryptpilot-verity -- format /path/to/dir --label env=prod -m metadata.fb
```

#### 方式 B：Go 代码实现

如需程序化控制，可在 Go 中逐文件计算 fs-verity 并序列化：

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

### 2. 反序列化与验证

加载元数据字节后，可执行三级完整性校验：

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

	// (a) 元数据根哈希 — 与构建期或签名时的可信值比对
	rootHash, _ := metadata.CalculateMetadataHash(data)
	expectedRootHash := "..." // 构建或签名时记录的可信哈希
	if rootHash != expectedRootHash {
		panic("元数据根哈希不匹配！")
	}

	// (b) 自验证 — 每个文件的描述符哈希和 Merkle 根哈希一致性
	for _, fi := range info.FileInfos {
		if err := fi.VerifySelf(); err != nil {
			fmt.Printf("%s: 验证失败: %v\n", fi.Path, err)
		}
	}

	// (c) 逐块验证 — 校验文件的指定数据块
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
			fmt.Printf("%s: 块 %d 已被篡改！\n", fi.Path, blockIndex)
		}
	}
}
```

## 包说明

| 包 | 说明 |
|----|------|
| `verity` | 核心 fs-verity：流式 hasher、Merkle tree、descriptor |
| `metadata` | FlatBuffers 序列化/反序列化目录完整性元数据 |

## 许可证

与父项目 cryptpilot 相同。见根目录下的 `LICENSE` 文件。
