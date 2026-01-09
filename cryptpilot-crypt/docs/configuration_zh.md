# 卷配置指南

本指南介绍 cryptpilot-crypt 数据卷加密的配置选项。

## 配置文件总览

默认配置目录为 `/etc/cryptpilot/volumes/`：

- 每个 `.toml` 文件定义一个加密卷
- 文件名可任意（如 `data0.toml`、`backup.toml`）
- 必须使用 `.toml` 扩展名

## 什么是"卷"

在 cryptpilot-crypt 中，"卷"是指 Linux 中任意一个需要加密的块设备（如 `/dev/nvme1n1p1`）。cryptpilot-crypt 可以对选定的任意卷进行初始化并管理，用于存储机密数据。

**主要操作：**

- **初始化** (`init`)：将设备格式化为加密的 LUKS2 卷（会抹除原有数据）
- **打开** (`open`)：解密卷并创建 `/dev/mapper/<volume-name>` 设备映射
- **关闭** (`close`)：锁定卷并移除设备映射

## 卷的配置

将卷配置文件放置在 `/etc/cryptpilot/volumes/`：

示例：`/etc/cryptpilot/volumes/data0.toml`

### 配置模板

参考模板：[otp.toml.template](../../dist/etc/volumes/otp.toml.template)

### 配置选项

```toml
# 解密后的卷名称
volume = "data0"

# 底层加密设备的路径
dev = "/dev/nvme1n1p1"

# 是否在启动时自动打开（默认：false）
auto_open = true

# 初始化时创建的文件系统
# 支持的值："swap"、"ext4"、"xfs"、"vfat"
# 如果设备已有数据则跳过
makefs = "ext4"

# 启用数据完整性保护（默认：false）
integrity = true

# 密钥提供者配置
[encrypt.otp]
```

**字段说明：**

- **`volume`**（必需）：卷名称，用于 `/dev/mapper/<volume>`
- **`dev`**（必需）：底层块设备路径
- **`auto_open`**（可选，默认：`false`）：通过 systemd 在启动时自动解密
- **`makefs`**（可选）：初始化时创建的文件系统类型
  - 支持：`"swap"`、`"ext4"`、`"xfs"`、`"vfat"`
  - 如设备已有数据则跳过
- **`integrity`**（可选，默认：`false`）：启用 dm-integrity 数据完整性保护
  - 每次读取时验证数据
  - 防止篡改（但无法防止回滚攻击）
- **`encrypt`**（必需）：密钥提供者配置（详见[密钥提供者](../../docs/key-providers_zh.md)）

## 启动时自动打开

要在系统启动时自动解密并打开卷：

1. 在卷配置中设置 `auto_open = true`
2. 启用 systemd 服务：

```sh
systemctl enable --now cryptpilot.service
```

该服务会自动打开所有 `auto_open = true` 的卷。

详细信息请参阅 [Systemd 服务](systemd-service_zh.md)。

## 使用示例

### 示例 1：临时交换分区（OTP）

```toml
volume = "swap0"
dev = "/dev/nvme1n1p1"
auto_open = true
makefs = "swap"

[encrypt.otp]
```

然后添加到 `/etc/fstab`：
```
/dev/mapper/swap0 none swap defaults 0 0
```

### 示例 2：持久化数据（KBS）

```toml
volume = "data0"
dev = "/dev/nvme1n1p2"
auto_open = true
makefs = "ext4"
integrity = true

[encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/data0-key"
```

然后添加到 `/etc/fstab`：
```
/dev/mapper/data0 /mnt/data0 ext4 defaults 0 2
```

### 示例 3：云托管密钥（KMS）

```toml
volume = "backup"
dev = "/dev/nvme1n1p3"
auto_open = false  # 仅手动打开
makefs = "xfs"

[encrypt.kms]
kms_instance_id = "kst-****"
client_key_id = "LTAI****"
client_key_password_from_kms = "alias/ClientKey_****"
```

需要时手动打开：
```sh
cryptpilot-crypt open backup
mount /dev/mapper/backup /mnt/backup
```

## 配置验证

检查配置有效性：

```sh
cryptpilot-crypt config check --keep-checking
```

选项：
- `--keep-checking`：即使发现错误也继续检查所有卷
- `--skip-check-passphrase`：跳过密码验证（更快，但不够全面）

## 参见

- [密钥提供者](../../docs/key-providers_zh.md) - 详细的密钥提供者配置
- [Systemd 服务](systemd-service_zh.md) - 启动时自动打开卷
- [开发指南](../../docs/development.md) - 构建和测试说明
- [主 README](../README.md) - 快速开始和使用示例
