# Boot Process of CryptPilot

Some features of CryptPilot rely on inserting specific code at certain points in the system's boot process. This document will detail these contents, making it convenient for you to troubleshoot errors when encountering boot issues.

A conventional Linux distribution boot process includes two stages: `Initrd` and `System Manager`. After the kernel completes initialization, components within the initrd begin running and prepare the final root file system. Subsequently, initrd hands over control to the System Manager component (usually systemd). The definitions of these two phases in this document are consistent with those in the [systemd documentation](https://www.freedesktop.org/software/systemd/man/latest/bootup.html).

## Initrd Stage

The work during the initrd stage relates to CryptPilot's system disk encryption feature; it is responsible for executing the disk decryption process.

We have created a dracut module named [[cryptpilot](file:///root/cryptpilot/target/debug/cryptpilot)](/dist/dracut/modules.d/91cryptpilot/module-setup.sh). Each time `/boot/initramfs*.img` is updated (for example, by updating the kernel or manually running `dracut -v -f`), the CryptPilot executable will also be copied into it.

> [!NOTE]
> If the system disk encryption feature is not used (i.e., the `/etc/cryptpilot/fde.toml` file is not created), this dracut module will not be enabled.

As described in the [systemd documentation](https://www.freedesktop.org/software/systemd/man/latest/bootup.html#Bootup%20in%20the%20initrd), the system startup process within the initrd is very similar to that of the System Manager phase, being controlled by systemd services as well. CryptPilot creates two services, placed both before and after `initrd-root-device.target`:

- [[cryptpilot-fde-before-sysroot.service](file:///root/cryptpilot/dist/dracut/modules.d/91cryptpilot/cryptpilot-fde-before-sysroot.service)](/dist/dracut/modules.d/91cryptpilot/cryptpilot-fde-before-sysroot.service): Starts before `initrd-root-device.target`, which decrypts the rootfs volume (if necessary) and performs measurement on its content. Additionally, it checks the data volume; if the data volume does not exist, it initializes a new data volume using the remaining space on the disk, which usually happens during the first system boot. If the data volume already exists, it will decrypt it.

- [[cryptpilot-fde-after-sysroot.service](file:///root/cryptpilot/dist/dracut/modules.d/91cryptpilot/cryptpilot-fde-after-sysroot.service)](/dist/dracut/modules.d/91cryptpilot/cryptpilot-fde-after-sysroot.service): Starts after `initrd-root-device.target`, responsible for mounting the data volume to the `/data` directory and handling some trivial mount tasks.

## System Manager Stage

In the System Manager stage, we rely on a systemd service named [cryptpilot.service](/dist/systemd/cryptpilot.service) to implement the Auto Open feature for encrypted data volumes. During system startup, this service checks configuration files within `/etc/cryptpilot/volumes/` and automatically opens encrypted data volumes according to the configurations.

If you need to use the Auto Open feature, please use the following command to make the service start at boot:

```sh
systemctl enable --now cryptpilot.service
```
