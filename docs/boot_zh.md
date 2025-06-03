# CryptPilot的启动流程

CryptPilot的部分功能依赖于在系统的启动过程的某些位置插入特定的代码。本文将详细介绍这部分内容，方便您在遇到启动问题时排查错误。

常规的Linux发行版启动包含`Initrd`和`System Manager`两个阶段。当kernel完成初始化后，initrd中的组件开始运行，并准备好最终的根文件系统。随后initrd将控制权交给system manager组件（通常是systemd）。本文中对于两个阶段的定义与[systemd文档](https://www.freedesktop.org/software/systemd/man/latest/bootup.html)一致。

## Initrd阶段

在initrd阶段的工作与CryptPilot的系统盘加密特性有关，它会负责执行的磁盘的解密过程。

我们创建了一个名为[`cryptpilot`](/dist/dracut/modules.d/91cryptpilot/module-setup.sh)的dracut module，每次更新`/boot/initramfs*.img`时（比如更新内核或者手动运行`dracut -v -f`），会将CryptPilot可执行文件也拷贝到其中。

> [!NOTE]
> 如果未使用系统盘加密特性（即没有创建`/etc/cryptpilot/fde.toml`文件），则不会启用该dracut module。

正如[systemd的文档](https://www.freedesktop.org/software/systemd/man/latest/bootup.html#Bootup%20in%20the%20initrd)中所描述，系统在initrd中的启动过程和System Manager阶段非常类似，也是由systemd服务来控制的。CryptPilot创建了两个服务，分别位于`initrd-root-device.target`的前面和后面：

- [`cryptpilot-fde-before-sysroot.service`](/dist/dracut/modules.d/91cryptpilot/cryptpilot-fde-before-sysroot.service)：在`initrd-root-device.target`之前启动，它将对rootfs卷进行解密（如果需要的话），并完成对其内容的度量。此外，它还将检查data卷，如果data卷不存在，它会用磁盘剩余的空间来初始化一个新的data卷，这通常发生在系统首次启动时。如果data卷已经存在，则它将解密data卷。

- [`cryptpilot-fde-after-sysroot.service`](/dist/dracut/modules.d/91cryptpilot/cryptpilot-fde-after-sysroot.service)：在`initrd-root-device.target`之后启动，它会负责将data卷挂载到`/data`目录中，并处理一些琐碎的挂载任务。

## System Manager阶段

在System Manager阶段，我们依赖于一个名为[cryptpilot.service](/dist/systemd/cryptpilot.service)的systemd服务，来实现加密数据卷的自动打开（Auto Open）特性。该服务会在系统启动过程中检查`/etc/cryptpilot/volumes/`中的配置文件，并根据配置自动打开加密数据卷。

如果你需要使用Auto Open特性，请使用以下命令配置该服务自动启动：

```sh
systemctl enable --now cryptpilot.service
```


