# 快速开始指南：cryptpilot-crypt

本指南将指导你设置用于运行时使用的加密数据卷。

## 前置条件

- 已安装 cryptpilot-crypt
- 要加密的块设备或分区（例如 `/dev/nvme1n1p1`）
- 设备应该未挂载且不在使用中

## 示例 1：使用 OTP 加密数据卷（易失性存储）

此示例使用一次性密码创建加密卷。数据是易失性的，关闭后将丢失。

### 步骤 1：创建分区

如果还没有分区，创建一个：

```sh
parted --script /dev/nvme1n1 \
    mktable gpt \
    mkpart part1 0% 100%
```

### 步骤 2：创建卷配置

在 `/etc/cryptpilot/volumes/data0.toml` 创建配置文件：

```sh
mkdir -p /etc/cryptpilot/volumes
cat << EOF > /etc/cryptpilot/volumes/data0.toml
volume = "data0"
dev = "/dev/nvme1n1p1"
auto_open = true
makefs = "ext4"
integrity = true

[encrypt.otp]
EOF
```

**配置说明：**

- `volume = "data0"`：卷名称（将创建 `/dev/mapper/data0`）
- `dev = "/dev/nvme1n1p1"`：底层块设备
- `auto_open = true`：启动时自动打开
- `makefs = "ext4"`：首次初始化时创建 ext4 文件系统
- `integrity = true`：启用 dm-integrity 数据完整性保护
- `[encrypt.otp]`：使用一次性密码（数据是易失性的）

### 步骤 3：检查配置

验证配置：

```sh
cryptpilot-crypt config check --keep-checking
```

### 步骤 4：初始化卷

初始化加密卷（仅首次需要）：

```sh
cryptpilot-crypt init data0
```

这将：
1. 使用 LUKS2 格式化设备
2. 创建文件系统（ext4）
3. 如果启用，设置 dm-integrity

### 步骤 5：打开卷

打开（解密）卷：

```sh
cryptpilot-crypt open data0
```

### 步骤 6：检查卷状态

验证卷已打开：

```sh
cryptpilot-crypt show
```

示例输出：

```
╭────────┬───────────────────┬─────────────────┬──────────────┬──────────────────┬──────────────┬────────╮
│ Volume ┆ Volume Path       ┆ Underlay Device ┆ Key Provider ┆ Extra Options    ┆ Initialized  ┆ Opened │
╞════════╪═══════════════════╪═════════════════╪══════════════╪══════════════════╪══════════════╪════════╡
│ data0  ┆ /dev/mapper/data0 ┆ /dev/nvme1n1p1  ┆ otp          ┆ auto_open = true ┆ Not Required ┆ True   │
│        ┆                   ┆                 ┆              ┆ makefs = "ext4"  ┆              ┆        │
│        ┆                   ┆                 ┆              ┆ integrity = true ┆              ┆        │
╰────────┴───────────────────┴─────────────────┴──────────────┴──────────────────┴──────────────┴────────╯
```

### 步骤 7：挂载并使用

挂载卷并开始使用：

```sh
mkdir -p /mnt/data0
mount /dev/mapper/data0 /mnt/data0
```

现在你可以在 `/mnt/data0` 中读写文件。

### 步骤 8：关闭卷

使用完毕后，卸载并关闭：

```sh
umount /mnt/data0
cryptpilot-crypt close data0
```

> [!WARNING]
> 使用 OTP 提供者时，关闭卷将永久擦除所有数据！OTP 仅用于临时/暂存存储。

## 示例 2：启动时自动打开

要在系统启动期间自动打开卷：

### 步骤 1：在配置中设置 auto_open

确保卷配置中有 `auto_open = true`（示例 1 中已设置）。

### 步骤 2：启用 systemd 服务

```sh
systemctl enable --now cryptpilot.service
```

### 步骤 3：添加到 /etc/fstab（可选）

为了自动挂载，添加到 `/etc/fstab`：

```sh
echo "/dev/mapper/data0 /mnt/data0 ext4 defaults 0 2" >> /etc/fstab
```

现在卷将在每次启动时自动解密和挂载。

## 示例 3：使用 KBS 的持久存储（生产环境）

对于生产工作负载，使用带有远程证明的密钥代理服务。

### 配置

```sh
cat << EOF > /etc/cryptpilot/volumes/data1.toml
volume = "data1"
dev = "/dev/nvme1n1p2"
auto_open = true
makefs = "ext4"
integrity = true

[encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/data1-key"
EOF
```

### 初始化并打开

```sh
cryptpilot-crypt config check --keep-checking
cryptpilot-crypt init data1
cryptpilot-crypt open data1
mkdir -p /mnt/data1
mount /dev/mapper/data1 /mnt/data1
```

### 工作原理

打开卷时：

1. 在 TEE 中生成证明证据
2. 将证据发送到 KBS
3. KBS 验证证据
4. 如果验证通过，KBS 返回解密密钥
5. 卷被解密并打开

## 示例 4：使用 KMS 的云托管密钥

对于阿里云用户，使用 KMS 进行集中式密钥管理。

### 配置

```sh
cat << EOF > /etc/cryptpilot/volumes/data2.toml
volume = "data2"
dev = "/dev/nvme1n1p3"
auto_open = true
makefs = "xfs"
integrity = false

[encrypt.kms]
kms_instance_id = "kst-****"
client_key_id = "LTAI****"
client_key_password_from_kms = "alias/ClientKey_****"
EOF
```

### 初始化并打开

```sh
cryptpilot-crypt config check --keep-checking
cryptpilot-crypt init data2
cryptpilot-crypt open data2
mkdir -p /mnt/data2
mount /dev/mapper/data2 /mnt/data2
```

## 示例 5：使用不同提供者的多个卷

你可以配置使用不同密钥提供者的多个卷：

```sh
# 卷 1：临时存储（OTP）
cat > /etc/cryptpilot/volumes/scratch.toml << EOF
volume = "scratch"
dev = "/dev/nvme1n1p1"
auto_open = true
makefs = "ext4"

[encrypt.otp]
EOF

# 卷 2：持久数据（KBS）
cat > /etc/cryptpilot/volumes/data.toml << EOF
volume = "data"
dev = "/dev/nvme1n1p2"
auto_open = true
makefs = "ext4"
integrity = true

[encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/data-key"
EOF

# 卷 3：备份存储（KMS）
cat > /etc/cryptpilot/volumes/backup.toml << EOF
volume = "backup"
dev = "/dev/nvme2n1"
auto_open = false
makefs = "xfs"

[encrypt.kms]
kms_instance_id = "kst-****"
client_key_id = "LTAI****"
client_key_password_from_kms = "alias/ClientKey_****"
EOF
```

初始化并打开所有卷：

```sh
cryptpilot-crypt init scratch
cryptpilot-crypt init data
cryptpilot-crypt init backup

systemctl enable --now cryptpilot.service  # 自动打开 scratch 和 data
cryptpilot-crypt open backup  # 手动打开 backup
```

## 示例 6：交换分区加密

创建加密的交换分区：

```sh
cat > /etc/cryptpilot/volumes/swap.toml << EOF
volume = "swap"
dev = "/dev/nvme1n1p4"
auto_open = true
makefs = "swap"

[encrypt.otp]
EOF
```

初始化、打开并激活：

```sh
cryptpilot-crypt init swap
cryptpilot-crypt open swap
swapon /dev/mapper/swap
```

添加到 `/etc/fstab`：

```sh
echo "/dev/mapper/swap none swap defaults 0 0" >> /etc/fstab
```

## 故障排除

### 配置检查失败

如果 `config check` 报告错误：

```sh
cryptpilot-crypt config check --keep-checking
```

常见问题：
- 缺少必需字段（`volume`、`dev`、`encrypt`）
- 无效的设备路径
- 无效的密钥提供者配置

### 初始化失败

如果 `cryptpilot-crypt init` 失败：

1. **检查设备存在**：`ls -l /dev/nvme1n1p1`
2. **检查设备未使用**：`lsblk`、`mount | grep nvme1n1p1`
3. **检查权限**：使用足够的权限运行
4. **检查密钥提供者**：确保提供者可达（KBS/KMS）

### 打开失败

如果 `cryptpilot-crypt open` 失败：

1. **检查卷已初始化**：`cryptpilot-crypt show`
2. **检查密钥提供者**：验证网络/证明正常工作
3. **检查设备**：确保底层设备可用
4. **检查日志**：`journalctl -u cryptpilot.service`

### 自动打开不工作

如果卷在启动时未打开：

1. **检查服务已启用**：`systemctl status cryptpilot.service`
2. **检查 auto_open 设置**：验证配置中有 `auto_open = true`
3. **检查服务日志**：`journalctl -u cryptpilot.service`
4. **检查网络**：对于远程提供者（KBS/KMS），确保网络已启动

## 下一步

- [配置指南](configuration_zh.md) - 详细配置选项
- [Systemd 服务](systemd-service_zh.md) - 启动时自动打开卷
- [密钥提供者](../../docs/key-providers_zh.md) - 密钥提供者配置详情
