# Development

## Build and Install

### RPM Package (RHEL/CentOS/Anolis)

It is recommended to build a RPM package and install it on your system.

```sh
make create-tarball rpm-build
```

Then install the rpm package on your system:

```sh
rpm --install /root/rpmbuild/RPMS/x86_64/cryptpilot-[0-9]*.rpm
```

### DEB Package (Debian/Ubuntu)

For Debian/Ubuntu systems, you can build a DEB package:

```sh
make create-tarball deb-build
```

Then install the deb package on your system:

```sh
sudo dpkg -i /tmp/cryptpilot_*.deb
```


### Test

We have provided some test cases. You can run them with the following command:

```sh
make run-test
```

