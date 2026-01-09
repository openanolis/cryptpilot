# 快速开始指南：cryptpilot-fde

本指南将指导你使用全盘加密来加密可启动的操作系统磁盘。

## 前置条件

- 已安装 cryptpilot-fde
- 可启动的 qcow2 磁盘镜像，或未挂载的真实磁盘

## 准备配置

加密之前，你需要准备一个至少包含一个 `fde.toml` 文件的配置目录。配置目录结构类似于 `/etc/cryptpilot/`。

在这个演示中，我们将使用带有硬编码密码短语的 `exec` 密钥提供者：

> [!IMPORTANT]
> 下面的 `exec` 密钥提供者仅用于演示目的。生产环境请使用 `kbs` 或 `kms`。

```sh
mkdir -p ./config_dir
cat << EOF > ./config_dir/fde.toml
[rootfs]
rw_overlay = "disk"

[rootfs.encrypt.exec]
command = "echo"
args = ["-n", "AAAaaawewe222"]

[data]
integrity = true

[data.encrypt.exec]
command = "echo"
args = ["-n", "AAAaaawewe222"]
EOF

tree ./config_dir
```

配置目录结构：

```txt
./config_dir
└── fde.toml
```

**配置说明：**

- `[rootfs]`：根文件系统配置
  - `rw_overlay = "disk"`：将可写覆盖层存储在数据分区上（重启后保留）
  - `encrypt.exec`：使用 exec 提供者，密码为 "AAAaaawewe222"
- `[data]`：数据分区配置
  - `integrity = true`：启用 dm-integrity 数据完整性保护
  - `encrypt.exec`：使用 exec 提供者，密码为 "AAAaaawewe222"

### 验证配置

检查配置是否有效：

```sh
cryptpilot-fde -c ./config_dir/ config check --keep-checking
```

## 示例 1：加密磁盘镜像文件

此示例展示如何加密现有的可启动磁盘镜像。

### 步骤 1：下载磁盘镜像

我们将使用阿里云 Linux 3 磁盘镜像：

```sh
wget https://alinux3.oss-cn-hangzhou.aliyuncs.com/aliyun_3_x64_20G_nocloud_alibase_20251030.qcow2
```

### 步骤 2：加密磁盘镜像

使用准备好的配置加密磁盘镜像：

```sh
cryptpilot-convert --in ./aliyun_3_x64_20G_nocloud_alibase_20251030.qcow2 \
    --out ./encrypted.qcow2 \
    -c ./config_dir/ \
    --rootfs-passphrase AAAaaawewe222
```

**加密过程中发生的事情：**

1. 读取原始磁盘镜像
2. 创建带有 dm-verity 的加密 rootfs 分区
3. 创建带有 dm-integrity 的加密数据分区
4. 将 cryptpilot-fde 安装到 initrd
5. 配置引导加载程序以支持加密启动
6. 将加密磁盘写入输出文件

**可选：** 可以在加密期间安装额外的软件包：

```sh
cryptpilot-convert --in ./source.qcow2 --out ./encrypted.qcow2 \
    -c ./config_dir/ \
    --rootfs-passphrase AAAaaawewe222 \
    --package /path/to/package.rpm
```

### 步骤 3：测试加密磁盘（可选）

启动虚拟机测试加密磁盘：

```sh
# 安装 qemu-kvm
yum install -y qemu-kvm

# 下载 cloud-init 的 seed 镜像
wget https://alinux3.oss-cn-hangzhou.aliyuncs.com/seed.img

# 启动虚拟机
/usr/libexec/qemu-kvm \
    -m 4096M \
    -smp 4 \
    -nographic \
    -drive file=./encrypted.qcow2,format=qcow2,if=virtio,id=hd0,readonly=off \
    -drive file=./seed.img,if=virtio,format=raw
```

> **登录凭据：** 用户名：`alinux`，密码：`aliyun`

**退出 QEMU：** 按 `Ctrl-A` 然后 `C` 进入 QEMU 控制台，然后输入 `quit`。

### 步骤 4：计算参考值

为了证明目的，计算加密参考值：

```sh
cryptpilot-fde show-reference-value --stage system --disk ./encrypted.qcow2
```

这将输出可上传到[参考值提供服务（RVPS）](https://github.com/confidential-containers/trustee/tree/main/rvps)的度量值。

### 步骤 5：上传并启动

将加密的磁盘镜像上传到云提供商（例如阿里云）并从其启动。

## 示例 2：仅度量 rootfs（不加密）

对于某些场景，你可能只需要对 rootfs 进行完整性保护和度量，而不需要加密。这种情况下，rootfs 使用 dm-verity 保护，但不加密。

> [!NOTE]
> 这种模式适用于以下场景：
> - rootfs 中不包含敏感数据
> - 只需要完整性验证和度量，不需要保密性
> - 减少启动时的性能开销

### 配置

创建不加密 rootfs 的配置：

```sh
mkdir -p ./config_dir
cat << EOF > ./config_dir/fde.toml
[rootfs]
rw_overlay = "disk"
# 注意：rootfs 段不包含 encrypt 配置

[data]
integrity = true

[data.encrypt.exec]
command = "echo"
args = ["-n", "AAAaaawewe222"]
EOF
```

**配置说明：**

- `[rootfs]`：根文件系统配置
  - `rw_overlay = "disk"`：将可写覆盖层存储在数据分区上
  - **不包含** `encrypt` 配置：rootfs 不加密，仅使用 dm-verity 完整性保护
- `[data]`：数据分区配置
  - `integrity = true`：启用 dm-integrity
  - `encrypt.exec`：数据分区仍然加密

### 加密数据分区（rootfs 不加密）

使用 `--rootfs-no-encryption` 参数：

```sh
cryptpilot-convert --in ./aliyun_3_x64_20G_nocloud_alibase_20251030.qcow2 \
    --out ./encrypted.qcow2 \
    -c ./config_dir/ \
    --rootfs-no-encryption
```

**发生的事情：**

1. rootfs 使用 dm-verity 进行完整性保护（不加密）
2. 数据分区正常加密
3. 系统启动时仍会进行度量和证明
4. rootfs 以只读方式挂载，通过 overlay 提供可写层

### 适用场景

这种配置适合以下情况：

- ✅ rootfs 中只包含公开的系统文件
- ✅ 需要验证系统完整性（防篡改）
- ✅ 需要远程证明确认系统未被修改
- ✅ 希望减少解密 rootfs 的性能开销
- ❌ 不适合 rootfs 中包含敏感配置或密钥的场景

## 示例 3：加密真实系统磁盘

对于生产系统，你需要加密真实磁盘。

> [!IMPORTANT]
> **不要加密正在启动的活动磁盘！**
> 
> 你必须：
> 1. 从实例解绑磁盘
> 2. 将其作为数据盘绑定到另一个实例
> 3. 加密它
> 4. 重新绑定到原始实例

### 步骤

1. **准备配置**（与上面相同）：

```sh
mkdir -p ./config_dir
cat << EOF > ./config_dir/fde.toml
[rootfs]
rw_overlay = "disk"

[rootfs.encrypt.exec]
command = "echo"
args = ["-n", "AAAaaawewe222"]

[data]
integrity = true

[data.encrypt.exec]
command = "echo"
args = ["-n", "AAAaaawewe222"]
EOF
```

2. **验证配置**：

```sh
cryptpilot-fde -c ./config_dir/ config check --keep-checking
```

3. **加密磁盘**（假设磁盘是 `/dev/nvme2n1`）：

```sh
cryptpilot-convert --device /dev/nvme2n1 \
    -c ./config_dir/ \
    --rootfs-passphrase AAAaaawewe222
```

4. **重新绑定磁盘**到原始实例并从其启动。

## 示例 4：使用 KBS 提供者（生产环境）

对于生产环境，使用带有远程证明的密钥代理服务。

### 配置

```sh
mkdir -p ./config_dir
cat << EOF > ./config_dir/fde.toml
[rootfs]
rw_overlay = "disk"

[rootfs.encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/rootfs-key"

[data]
integrity = true

[data.encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/data-key"
EOF
```

### 加密

```sh
# 磁盘镜像
cryptpilot-convert --in ./original.qcow2 --out ./encrypted.qcow2 \
    -c ./config_dir/ --rootfs-passphrase <实际-rootfs-密钥>

# 真实磁盘
cryptpilot-convert --device /dev/nvme2n1 \
    -c ./config_dir/ --rootfs-passphrase <实际-rootfs-密钥>
```

### 启动过程

启动时，系统将：

1. 在 TEE 中生成证明证据
2. 将证据发送到 KBS
3. KBS 验证证据
4. 如果验证通过，KBS 返回解密密钥
5. 系统解密并启动

## 示例 5：使用 KMS 提供者（云托管）

对于阿里云用户，使用 KMS 进行集中式密钥管理。

### 配置

```sh
mkdir -p ./config_dir
cat << EOF > ./config_dir/fde.toml
[rootfs]
rw_overlay = "disk"

[rootfs.encrypt.kms]
kms_instance_id = "kst-****"
client_key_id = "LTAI****"
client_key_password_from_kms = "alias/ClientKey_****"

[data]
integrity = true

[data.encrypt.kms]
kms_instance_id = "kst-****"
client_key_id = "LTAI****"
client_key_password_from_kms = "alias/ClientKey_****"
EOF
```

### 加密

```sh
cryptpilot-convert --in ./original.qcow2 --out ./encrypted.qcow2 \
    -c ./config_dir/ --rootfs-passphrase <从-kms-获取>
```

## 在 Docker 中运行

如果你不在[支持的发行版](#支持的发行版)上，可以使用 Docker：

### 步骤 1：加载 NBD 内核模块

```sh
modprobe nbd max_part=8
```

### 步骤 2：创建容器

```sh
docker run -it --privileged --ipc=host \
    -v /run/udev/control:/run/udev/control \
    -v /dev:/dev \
    alibaba-cloud-linux-3-registry.cn-hangzhou.cr.aliyuncs.com/alinux3/alinux3:latest bash
```

> **注意：** 额外的参数（`--privileged --ipc=host -v /run/udev/control:/run/udev/control -v /dev:/dev`）是为了使 `/dev` 在容器中正常工作。

### 步骤 3：安装 cryptpilot-fde

在容器内，从 [Release 页面](https://github.com/openanolis/cryptpilot/releases) 下载并安装 cryptpilot-fde：

```sh
# 下载最新版本的 RPM 包
wget https://github.com/openanolis/cryptpilot/releases/download/vX.Y.Z/cryptpilot-fde-X.Y.Z-1.x86_64.rpm

# 安装
rpm -ivh cryptpilot-fde-X.Y.Z-1.x86_64.rpm
```

> **提示：** 将 `X.Y.Z` 替换为实际的版本号。

### 步骤 4：运行 cryptpilot 命令

```sh
cryptpilot-fde --help
cryptpilot-convert --help
```

现在你可以在容器内运行任何 cryptpilot-fde 命令。

## 故障排除

### 配置检查失败

如果 `config check` 报告错误：

```sh
cryptpilot-fde -c ./config_dir/ config check --keep-checking
```

常见问题：
- 配置中缺少必需字段
- 密钥提供者设置无效
- 文件路径不正确

### 转换失败

如果 `cryptpilot-convert` 失败：

1. **检查磁盘格式**：磁盘镜像仅支持 qcow2 格式
2. **检查磁盘大小**：确保有足够空间用于加密开销
3. **对于真实磁盘**：确保磁盘未挂载且不在使用中
4. **设备已存在错误**：如果出现类似 `/dev/system: already exists in filesystem` 的错误，可能是上次 convert 失败遗留的，尝试 `dmsetup remove_all` 清除
5. **查看日志**：最后一次 convert 的详细日志保存在 `/tmp/.cryptpilot-convert.log`

### 启动失败

如果加密系统启动失败：

1. **检查密钥提供者**：确保网络/证明正常工作
2. **检查参考值**：验证度量值与预期值匹配
3. **检查控制台输出**：查找启动期间的错误消息

## 下一步

- [配置指南](configuration_zh.md) - 详细配置选项
- [启动过程](boot_zh.md) - cryptpilot-fde 如何与启动集成
- [密钥提供者](../../docs/key-providers_zh.md) - 密钥提供者配置详情
- [cryptpilot-enhance](cryptpilot_enhance_zh.md) - 加密前加固镜像

## 参见

- [cryptpilot-crypt 快速开始](../../cryptpilot-crypt/docs/quick-start_zh.md) - 加密数据卷
- [开发指南](../../docs/development.md) - 构建和测试说明
