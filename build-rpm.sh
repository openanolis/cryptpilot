#!/bin/bash

ARCH=${ARCH:-x86_64}

mkdir -p dist/build

sudo -E docker build -t cryptpilot-rpm --network=host ./dist/ -f ./Dockerfile.rpm
sudo -E docker run -it --rm -v $(pwd)/dist/build/:/dist/build/ \
    cryptpilot-rpm \
    sh -c "cp /root/rpmbuild/RPMS/${ARCH}/* /dist/build/"
