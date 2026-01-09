# FDE 配置指南

本指南介绍 cryptpilot-fde 全盘加密（Full Disk Encryption）的配置选项。

## 配置文件总览

默认配置目录为 `/etc/cryptpilot/`：

- **`global.toml`**：全局配置（可选），参见 [global.toml.template](../../dist/etc/global.toml.template)
- **`fde.toml`**：FDE 配置，包含 rootfs 和 data 卷的配置

## FDE 配置

系统盘加密（全盘加密）是指将整个系统盘进行加密，该方案能够通过加密和完整性保护机制对根分区提供保护，并且 cryptpilot-fde 还能够实现对根文件系统的度量，用于远程证明。

加密后的系统盘包含两个主要卷：
- **Rootfs 卷**：只读的根文件系统
- **Data 卷**：可读写的数据分区

### 配置文件结构

参考模板：[fde.toml.template](../../dist/etc/fde.toml.template)

一个基础的 FDE 配置文件必须包含 `[rootfs]` 和 `[data]` 两个配置项。

### Rootfs 卷配置

rootfs 卷存放只读的根分区文件系统。对该文件系统的加密是可选的，但不管是否开启加密，在启动时该卷都会被度量，并基于 dm-verity 防止数据被修改。

在启动阶段，一个基于 overlayfs 的覆盖层将被覆盖在只读的根文件系统上，允许在根分区上做临时性的写入修改。

**配置选项：**

```toml
[rootfs]
# 覆盖层的存储位置："disk" 或 "ram"
# - "disk": 存储到 data 卷上（重启后保留）
# - "ram": 存储在内存中（重启后清除）
rw_overlay = "disk"

# 加密配置（可选）
# 如不指定，则根分区不加密（但仍受 dm-verity 保护）
[rootfs.encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/rootfs-key"
```

**字段说明：**

- **`rw_overlay`**（可选，默认：`"disk"`）：覆盖层存储位置
  - `"disk"`：存储到 data 卷（重启后保留）
  - `"ram"`：存储在内存（重启后清除）

- **`encrypt`**（可选）：rootfs 卷的密钥提供者配置
  - 如不指定，根分区不加密（但仍有 dm-verity 完整性保护）
  - 详见[密钥提供者](../../docs/key-providers_zh.md)文档

#### 度量与证明

##### 度量原理

cryptpilot-fde 使用远程证明（Remote Attestation）技术来实现对根文件系统的度量：

1. 根文件系统的预期值被记录在 initrd 镜像中
2. initrd 自身的度量值被记录在不可回滚的 EventLog (CCEL) 中
3. dm-verity 机制确保根文件系统的完整性
4. EventLog 可用于本地验证或远程证明验证

##### 使用 KBS 进行证明

在启动过程中，如果使用 `kbs` 作为密钥提供者，访问 KBS 服务获取解密密钥时会自动携带度量信息。KBS 服务的拥有者可以通过配置对应的[远程证明策略](https://github.com/openanolis/trustee/blob/main/attestation-service/docs/policy.md)加以验证，从而实现 CVM 启动的全链路可信。

### Data 卷配置

data 卷使用系统盘上剩余可用空间，包含一个加密的可读写文件系统。在系统启动过程中，该卷会被解密并挂载到 `/data` 位置。

**配置选项：**

```toml
[data]
# 开启数据完整性保护
integrity = true

# 加密配置（必需）
[data.encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/data-key"
```

**字段说明：**

- **`integrity`**（可选，默认：`false`）：开启 dm-integrity 数据完整性保护
  - 开启后，每次读取数据时都会进行校验
  - 防止数据篡改（但无法防止回滚攻击）

- **`encrypt`**（必需）：data 卷的密钥提供者配置
  - 详见[密钥提供者](../../docs/key-providers_zh.md)文档

## 配置验证

在使用前检查配置有效性：

```sh
cryptpilot-fde -c /path/to/config config check --keep-checking
```

选项：
- `--keep-checking`：即使发现错误也继续检查所有配置

## 参见

- [密钥提供者](../../docs/key-providers_zh.md) - 详细的密钥提供者配置
- [启动过程](boot_zh.md) - cryptpilot-fde 如何集成到系统启动
- [cryptpilot-enhance](cryptpilot_enhance_zh.md) - 磁盘加固工具
- [开发指南](../../docs/development.md) - 构建和测试说明
