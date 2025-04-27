# Development

## Build and Install

It is recommended to build a RPM package and install it on your system.

```sh
make rpm-build
```

Then install the rpm package on your system:

```sh
rpm --install /root/rpmbuild/RPMS/x86_64/cryptpilot-*.rpm
```


## Test

We have provided some test cases. You can run them with the following command:

```sh
make run-test
```

