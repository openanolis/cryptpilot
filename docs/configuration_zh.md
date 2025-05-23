# 配置说明

Cryptpilot可以通过配置文件来加密选项。配置文件采用TOML格式。

> 有关TOML语法的说明，请参考: https://toml.io/en/

## 配置文件总览

Cryptpilot默认配置文件目录为`/etc/cryptpilot/`，该目录下主要包含以下配置文件：

- `${config_dir}/global.toml`：全局配置，请参考模板 [global.toml.template](/dist/etc/global.toml.template)

- `${config_dir}/fde.toml`：系统盘加密配置，请参考[系统盘加密](#系统盘加密)章节

- `${config_dir}/volumes/`：存放数据卷配置的目录，每个配置文件对应一个数据卷。请参考[数据盘加密](#数据盘加密)章节


## 数据盘加密

### 什么是“卷”

在CryptPilot中，“卷”是指Linux中的任意一个需要加密的块设备（如/dev/nvme1n1p1），CryptPilot工具可以对选定的任意卷进行初始化，并在后续的流程中使用该卷存储机密数据。

数据盘加密的过程，即是将物理数据盘（或者数据盘上的某个物理分区）看作一个卷，并使用CryptPilot对其进行加密的过程。

卷的主要操作有：
- **初始化**（init）：对卷进行初始化，使其能够用于存储加密数据，这会抹除掉卷上的原始数据，并创建一个空白内容的加密卷
- **打开**（open）：对已经初始化的卷使用配置好的凭据进行解密，并在`/dev/mapper/${volume-name}`创建一个承载明文的虚拟块设备。在该块设备上写入的内容都将被加密后存储到实际的物理块设备上，
- **关闭**（close）：关闭一个指定的卷

### 卷的配置

使用CryptPilot定义卷时，首先需要在配置文件目录中的`${config_dir}/volumes/`放置一个对应的配置文件。例如`${config_dir}/volumes/example.toml`

这里是一个使用一次性密码加密的卷的配置文件示例：[otp.toml.template](/dist/etc/volumes/otp.toml.template)

> 注意：配置文件名必须以`.toml`结尾，且内容为TOML格式，非`.toml`结尾的文件将被忽略。建议将配置文件的名称和卷名保持一致，但这不是强制性的。

每个卷包含以下配置项：

```toml
# The name of resulting volume with decrypted data, which will be set up below `/dev/mapper/`.
volume = "data0"
# The path to the underlying encrypted device.
dev = "/dev/nvme1n1p1"
# Whether or not to open the LUKS2 device and set up mapping during booting. The default value is false.
auto_open = true
# The file system to initialize on the volume. Allowed values are ["swap", "ext4", "xfs", "vfat"]. If is not specified, or the device is not "empty", i.e. it contains any signature, the operation will be skipped.
makefs = "ext4"
# Whether or not to enable support for data integrity. The default value is false. Note that integrity cannot prevent a replay (rollback) attack.
integrity = true

# One Time Password (Temporary volume)
[encrypt.otp]
```

- `name`：卷的名称，用于标识卷。
- `dev`：卷对应的底层块设备的路径。
- `auto_open`：（可选，默认为`false`）表示是否在启动过程中自动打开该卷，该选项可被用于配合`/etc/fstab`使用实现加密卷上文件系统的自动挂载。
- `makefs`：（可选）表示在初始化过程中，是否自动创建卷文件系统，支持的选项有`"swap"`, `"ext4"`, `"xfs"`, `"vfat"`。
- `integrity`：（可选，默认为`false`）表示是否开启数据完整性保护，开启后，每次读取数据时都会进行校验，以保护数据完整性。
- `encrypt`：表示该卷加密使用的凭据存储类型，请参考[凭据存储类型](#凭据存储类型)章节


## 系统盘加密

系统盘加密，也称为全盘加密（Full Disk Encryption, FDE）是指将整个系统盘进行加密，该方案能够通过加密和完整性保护机制对根分区提供保护，并且CryptPilot还能够实现对根文件系统的度量。

使用CryptPilot加密后的系统盘是一个GPT分区的磁盘，包含两个主要的卷。分别是一个只读的rootfs卷，和一个可读写的data卷。rootfs卷和data卷可以分别配置不同的密码。

您可以参照[README.md](README.md)中的步骤使用CryptPilot对系统盘进行加密。

### 配置文件说明

这里有一个配置文件的参考模板 [fde.toml.template](/dist/etc/fde.toml.template)。

一个基础的系统盘加密配置文件至少包含`[rootfs]`和`[data]`两个配置项，分别对应对rootfs卷和data卷的配置。

#### rootfs卷

rootfs卷存放了只读的根分区文件系统，对该文件系统的加密是可选的。但不管是否开启加密，在启动时该卷都会被度量，并基于dm-verity防止数据被修改。在启动阶段，一个基于overlayfs的覆盖层将被覆盖在只读的根文件系统上，从而允许您在根分区上做临时性的写入修改，这些写入修改将不会破坏只读层，也不会影响只读根分区的度量。

rootfs卷包含如下配置项：

- `rw_overlay`：（可选，默认为`disk`）根文件系统之上的覆盖层的存储位置，可以是`disk`或者`ram`。当为`disk`时，覆盖层将落盘存储到磁盘的data卷上（见下文中的[data卷](#data卷)）。当为`ram`时，覆盖层将保留在内存中，且在实例重启或者关机时被清除。

- `encrypt`：（可选，默认为不加密）rootfs卷的凭据存储类型。该字段是可选的，如不指定则默认不对根分区进行加密。该字段的配置方式请参考[凭据存储类型](#凭据存储类型)，此处配置支持除`otp`之外的所有的凭据存储类型。

##### 度量

###### 度量原理

CryptPilot使用远程证明（Remote Attestation）技术来实现对根文件系统的度量，该特性依赖于系统中运行的Attestation-Agent服务，通过将根文件系统的度量值记录在不可回滚的EventLog中，配合内核的dm-verity机制，实现根文件系统的完整性保护。

在进入系统后，可以通过`/run/attestation-agent/eventlog`检查CryptPilot记录的EventLog：

```txt
# cat /run/attestation-agent/eventlog
INIT sha384/000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
cryptpilot.alibabacloud.com load_config {"alg":"sha384","value":"b8635580d85cb0a2b5896664eb795cadb99a589783817c81e263f6752f2a735d2705b4638947de3d947231b76b5a1877"}
cryptpilot.alibabacloud.com fde_rootfs_hash a3f73f5b995e7d8915c998d9f1e56b0e063a6e20c2bbb512e88e8fbc4e8f2965
cryptpilot.alibabacloud.com initrd_switch_root {}
```

如上所示，在CryptPilot启动过程中，共会记录三个EventLog：

| Domain | Operation | 示例值 | 描述 |
| --- | --- | --- | --- |
| cryptpilot.alibabacloud.com | load_config | `{"alg":"sha384","value":"b8635580d85cb0a2b5896664eb795cadb99a589783817c81e263f6752f2a735d2705b4638947de3d947231b76b5a1877"}` | CryptPilot所使用的配置文件的hash值 |
| cryptpilot.alibabacloud.com | fde_rootfs_hash | `a3f73f5b995e7d8915c998d9f1e56b0e063a6e20c2bbb512e88e8fbc4e8f2965` | 解密后启动的rootfs卷的度量值 |
| cryptpilot.alibabacloud.com | initrd_switch_root | `{}` | 一个事件记录，用于标识系统当前已经从initrd阶段切换到真实的系统中，该项的值始终为`{}` |

进入系统后，业务可以基于该度量机制产生的EventLog，对系统启动过程进行本地验证，或者通过远程证明的方式提供给可信实体进行验证。

###### 使用`kbs`作为凭据存储类型

在启动过程中，如果使用`kbs`作为rootfs卷或者data卷的存储类型，那么在访问KBS服务获取卷解密凭据时，会自动携带度量信息。KBS服务的拥有者可以通过配置对应的[远程证明策略](https://github.com/openanolis/trustee/blob/b1a278a4360b9b47f82001b5c3d350b8c154acf5/attestation-service/docs/policy.md)加以验证，从而实现CVM启动的全链路可信。


#### data卷

data卷是系统盘上剩余可用空间组成的一个加密卷，包含一个可读写的Ext4文件系统。在系统启动过程中，该卷会被解密，并且在进入系统后，该卷会被挂载到`/data`位置上。任何data卷上写入的数据，都会被加密后落盘。用户可以将其数据文件写入到此处，在实例重新启动后，数据不会丢失。

data卷包含如下配置项：

- `integrity`：（可选，默认为`false`），表示是否开启数据完整性保护。开启后，每次从盘上读取数据时，都会对数据进行校验，该选项可以防止数据篡改行为。
- `encrypt`：data卷的凭据存储类型。该字段的配置方式请参考[凭据存储类型](#凭据存储类型)，此处配置支持除`otp`之外的所有的凭据存储类型。


## 凭据存储类型

CryptPilot通过模块化设计，支持从多种凭据存储类型中获取卷的解密密钥。在本文档中记录的是已经实现的一些凭据存储类型，随着版本的迭代，支持的存储类型将会增加。

### `[encrypt.otp]`：一次性密码 OTP

这是一种特殊的凭据存储类型，指示CryptPilot使用安全随机数生成的一次性密码来加密卷。该密码将是一次性的，这意味着使用该凭据存储类型的卷无需初始化过程，并且每一次打开都将自动触发数据擦除操作。因此每次打开时都是一个全新的卷，适用于需要临时存储加密数据的场景。

配置文件示例：[otp.toml.template](/dist/etc/volumes/otp.toml.template)

### `[encrypt.kbs]`：Key Broker Service（KBS）

表示凭据托管在[Key Broker Service（KBS）](https://github.com/openanolis/trustee/tree/main/kbs#key-broker-service)中，并且使用远程证明（Remote Attestation）进行认证以获取用于解密卷的凭据。

配置文件示例：[kbs.toml.template](/dist/etc/volumes/kbs.toml.template)

### `[encrypt.kms]`：密钥管理服务 KMS（Access Key）

表示凭据托管在[阿里云密钥管理服务KMS](https://yundun.console.aliyun.com/)中，并且使用给定的Access Key进行身份验证以获取凭据。

配置文件示例：[kms.toml.template](/dist/etc/volumes/kms.toml.template)

### `[encrypt.oidc]`：密钥管理服务 KMS（OIDC）

表示凭据托管在[阿里云密钥管理服务KMS](https://yundun.console.aliyun.com/)中。并且需要通过OIDC（OpenID Connect）认证协议行身份验证以获取凭据。

该凭据存储类型允许配置一个提供OIDC token的外部程序，CryptPilot将执行该外部程序以获得OIDC token，并用于后续从KMS获取凭据的过程。

配置文件示例：[oidc.toml.template](/dist/etc/volumes/oidc.toml.template)

### `[encrypt.exec]`：提供密码的可执行程序（EXEC）

这是一个特殊的凭据存储类型，指示CryptPilot通过执行一个外部程序并从该外部程序的标准输出（stdout）凭据中获取用于解密卷的凭据。

> **注意**
> 该外部程序的标准输出数据将原封不动地被当作解密凭据，期间不会进行裁剪、或者字符串转换，也不涉及base64解码。因此您需要确保没有多余的不可见字符如回车符和空格符。

配置文件示例：[exec.toml.template](/dist/etc/volumes/exec.toml.template)
