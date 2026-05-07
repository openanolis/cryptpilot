# cryptpilot-verity: 用户态文件系统完整性保护

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

`cryptpilot-verity` 是一个用于为只读数据目录生成、验证和使用 fs-verity 风格完整性元数据的命令行工具。它可以被视为针对通用只读目录树定制的 fs-verity 的用户空间实现。
它会计算整个数据集的完整性"根哈希值"，以 FlatBuffers 格式存储每个文件的验证元数据，并可以通过强制执行文件系统级完整性检查的 FUSE 文件系统挂载数据目录，在读取时进行验证。

## 与 dm-verity、fs-verity 和 composefs 的关系

`cryptpilot-verity` 在概念上类似于 Linux 的 dm-verity 和内核内置的 fs-verity 特性，但完全在用户空间运行，并专注于目录树而不是块设备或单个文件。

- **与 [dm-verity](https://docs.kernel.org/admin-guide/device-mapper/verity.html) 相比**：dm-verity 在块层保护块设备，而 `cryptpilot-verity` 通过 FUSE 文件系统保护只读目录树。无需配置或管理专用的 verity 块设备。
- **与内核内置的 [fs-verity](https://docs.kernel.org/filesystems/fsverity.html) 相比**：fs-verity 目前仅支持有限的文件系统（ext4、f2fs、btrfs），不适用于用户空间文件系统，如基于对象存储的 FUSE 挂载或基于 virtio-fs 的共享。fs-verity 还在**每个文件粒度**上工作，**不**保护文件系统元数据（目录项、路径与 inode 之间的链接）。控制底层存储的攻击者可以更改目录结构，使上层打开一个**未启用** fs-verity 的不同文件。相比之下，`cryptpilot-verity` 测量并验证**整个目录树**，包括路径到受保护文件的映射。
- **与 [composefs](https://github.com/composefs/composefs) 相比**：composefs 专注于从内容寻址对象组合不可变的文件系统树，主要用于容器镜像。它充分利用现有的内核特性：使用 EROFS 镜像存储路径和目录元数据，overlayfs 将 EROFS 视图与底层 `objects/` 目录组合，可以为 EROFS 块设备启用 dm-verity 以保护元数据完整性。但是，实际的文件负载仍然依赖于托管对象目录的文件系统（例如 ext4）来启用 fs-verity。将普通目录转换为 composefs 还需要将布局重写为对象存储并构建 EROFS 镜像。相比之下，`cryptpilot-verity` 特意设计得很轻量：它不会修改原始文件或目录布局，只添加一个 FlatBuffers 元数据文件（`cryptpilot-verity.metadata.fb`），记录用于验证的 Merkle 树和描述符。

CLI 接口和子命令设计有意与 `veritysetup` 工具类似，以便熟悉 dm-verity 的用户容易上手。

## 威胁模型

`cryptpilot-verity` 主要为机密计算风格的部署场景设计，其中虚拟机挂载的只读数据目录的后备存储是**不受信任的**（例如，主机端磁盘、对象存储如 OSS、远程 NAS 或由不受信任的存储支持的 virtio-fs 共享）。攻击者可能随时修改底层存储，但无法直接破坏 guest 内核。

- **我们防御的内容**：
  - 对受保护目录树中文件内容的离线或在线篡改。
  - 试图通过更改目录结构将受保护的文件替换为未受保护文件的攻击。
  - 试图逃离预期树或重定向文件访问的路径遍历和符号链接技巧。实现依赖于 Rust 的类型系统以及诸如 `openat2()` + `RESOLVE_BENEATH` 之类的内核特性来确保路径保持受限。
  - 运行时读取时篡改：在将数据返回给调用者之前，使用 Merkle 树重新验证数据，与 fs-verity 机制非常相似。

- **verity 测量覆盖的内容**：
  - 受保护文件的**文件内容**。
  - **文件路径**及其与受保护内容的关联，以便可以检测到改变路径指向的文件。

- **未覆盖的内容**：
  - POSIX 元数据，如权限位、所有权（`uid`、`gid`）和时间戳。
  - 挂载选项、内核端权限检查或更高级别的应用程序逻辑。
  - 从未包含在格式化元数据中的文件或目录的完整性；实际上，此类路径会被忽略，并且不会出现在公开的文件系统视图中。同样，如果元数据中包含的文件后来从底层文件系统中删除，这将被视为不存在而不是主动篡改，本身不会触发完整性失败。
  - 标签（格式时附加的键值元数据）。标签存储在元数据文件中但不受 root hash 完整性保护。
  
## 安全注意事项

- 该工具假设数据目录在格式化后是**只读的**；格式化后修改底层文件将导致验证失败。
- FUSE 层在读取时执行文件系统级验证，如果完整性检查失败则返回 I/O 错误。
- 元数据文件本身的完整性不需要单独保护：只要预期的根哈希受到保护，对元数据的任何篡改都会在重新计算哈希时被检测到。
- 始终保护预期的根哈希免遭篡改；它构成验证的信任锚点，可以使用 TPM 测量或机密计算 TEE 内的动态证明等机制进行保护。

## 功能特性

- **Format（格式化）**：扫描数据目录并计算 fs-verity 描述符、Merkle 树和全局根哈希。
- **Verify（验证）**：重新计算元数据哈希并将其与预期的根哈希进行比较。
- **Dump（转储）**：检查元数据文件或仅打印根哈希以进行调试或集成。
- **Open（打开）**：通过 `verity-fuse` 挂载数据目录并启用访问时验证。
- **Close（关闭）**：卸载先前挂载的 verity-fuse 文件系统。

## 高级工作流程

1. **格式化数据目录**
   - 遍历目录树。
   - 为每个文件计算 fs-verity 描述符和 Merkle 树。
   - 将完整的元数据（描述符、Merkle 树、描述符哈希）存储在 FlatBuffers 文件中。
   - 从元数据的最小视图派生确定性的元数据哈希（根哈希）。

2. **稍后验证完整性**
   - 读取元数据文件并重新计算元数据哈希。
   - 将重新计算的哈希与您提供的预期根哈希进行比较。

3. **使用验证挂载**
   - 使用元数据创建 `verity-fuse` 文件系统。
   - 在将数据返回给调用者之前，每次读取都会根据 Merkle 树进行验证。

## 命令

所有命令都是 `cryptpilot-verity` 二进制文件的子命令。运行 `cryptpilot-verity --help` 或 `cryptpilot-verity <subcommand> --help` 以获取详细信息。

### `format`

```bash
cryptpilot-verity format <DATA_DIR> [--metadata <METADATA_PATH>] [--force] [--label key=value]... --hash-output <HASH_OUTPUT>
```

- **目的**：为给定的数据目录生成 fs-verity 元数据和根哈希。
- **参数**：
  - `<DATA_DIR>`：要计算参考值的数据目录路径。
  - `--metadata, -m` **[可选]**：输出元数据文件（FlatBuffers 编码）的路径。如果未指定，默认为 `<DATA_DIR>/cryptpilot-verity.metadata.fb`。
  - `--hash-output`：写入根哈希的路径（使用 `-` 表示标准输出）。
  - `--force` **[可选]**：覆盖目标路径上的现有元数据文件。用于重新格式化或对已格式化目录进行第三方审计。
  - `--label key=value` **[可选，可重复]**：为元数据附加标签。标签是键值对（Docker 风格），存储在元数据文件中但不参与 root hash 计算。

### `verify`

```bash
cryptpilot-verity verify <DATA_DIR> <HASH> [--metadata <METADATA_PATH>] [--metadata-only]
```

- **目的**：验证数据目录的元数据是否与预期的根哈希匹配。
- **参数**：
  - `<DATA_DIR>`：要验证的数据目录路径。
  - `<HASH>`：预期的根哈希（十六进制编码）。
  - `--metadata, -m` **[可选]**：元数据文件的路径。如果未指定，默认为 `<DATA_DIR>/cryptpilot-verity.metadata.fb`。
  - `--metadata-only` **[可选]**：仅验证元数据完整性而不读取实际文件。启用时，仅检查元数据哈希是否与预期的根哈希匹配并验证元数据自一致性，而不验证各个文件内容是否与其描述符匹配。

### `dump`

```bash
cryptpilot-verity dump <DATA_DIR> --print-metadata
cryptpilot-verity dump --metadata <METADATA_PATH> --print-root-hash
cryptpilot-verity dump <DATA_DIR> --print-label <KEY>
cryptpilot-verity dump <DATA_DIR> --print-labels
```

- **目的**：检查元数据和/或仅打印根哈希。
- **参数**：
  - `<DATA_DIR>` **[可选]**：从中读取元数据的数据目录路径。必须指定 `<DATA_DIR>` 或 `--metadata` 之一（不需要同时指定两者）。如果提供 `<DATA_DIR>` 而未提供 `--metadata`，则从 `<DATA_DIR>/cryptpilot-verity.metadata.fb` 读取。
  - `--metadata` **[可选]**：直接读取的元数据文件路径。必须指定 `--metadata` 或 `<DATA_DIR>` 之一（不需要同时指定两者）。
  - `--print-metadata`：打印完整的解码元数据（必须指定此项或 `--print-root-hash`）。
  - `--print-root-hash`：仅打印根哈希（必须指定此项或 `--print-metadata`）。
  - `--print-label <KEY>`：输出指定标签键的值。如果键不存在则报错退出。
  - `--print-labels`：输出所有标签（每行一个 `key=value`）。如果未设置标签则输出 `(no labels)`。

### `open`

```bash
cryptpilot-verity open <DATA_DIR> <MOUNT_POINT> <HASH> [--metadata <METADATA_PATH>]
```

- **目的**：将数据目录挂载为启用验证的 verity-fuse 文件系统。
- **参数**：
  - `<DATA_DIR>`：要挂载的数据目录路径（必须与元数据匹配）。
  - `<MOUNT_POINT>`：FUSE 文件系统的目标挂载点。
  - `<HASH>`：预期的根哈希；用于在挂载前验证元数据。
  - `--metadata, -m` **[可选]**：元数据文件的路径。如果未指定，默认为 `<DATA_DIR>/cryptpilot-verity.metadata.fb`。

### `close`

```bash
cryptpilot-verity close <MOUNT_POINT>
```

- **目的**：卸载先前使用 `open` 挂载的 verity-fuse 文件系统。
- **参数**：
  - `<MOUNT_POINT>`：要卸载的挂载点。

## 元数据格式

元数据使用在 `src/metadata/metadata.fbs` 中定义的 FlatBuffers 模式进行存储和使用。生成的 FlatBuffers 文件（通常名为 `cryptpilot-verity.metadata.fb`）是 `cryptpilot-verity` 用于验证和挂载的内容。

各个文件的哈希算法与 Linux 内核的 fs-verity 实现完全兼容（默认使用 SHA-256 哈希，空盐和 4096 字节块）。这意味着对于任何给定的文件，`cryptpilot-verity` 计算的 fs-verity 描述符哈希与内核的 `FS_IOC_ENABLE_VERITY` ioctl 使用相同参数产生的结果完全匹配，也与 [fsverity-utils](https://git.kernel.org/pub/scm/fs/fsverity/fsverity-utils.git/) 工具集中 `fsverity digest` 命令的输出匹配。

元数据文件存储每个文件的 Merkle 树和描述符。根据经验，元数据大小约为总数据目录大小的 **1/128**（例如，1 GiB 的数据目录通常产生约 8 MiB 的元数据）。确切大小取决于文件数量和大小分布，但对于文件大于几个块的典型工作负载，此比率成立。
