#!/usr/bin/env python3

# requires current fs to have support for verity, meaning: ext4, with the right flag:
# sudo tune2fs -O verity /dev/sda1

# requires the fsverity userspace util:
# sudo apt install fsverity

# using a temporary fs would be a lot nicer, maybe take inspiration from xfstests:
# "[4/7] generic: test fs-verity descriptor validation"
# https://patchwork.kernel.org/project/fstests/patch/20181210222142.222342-5-ebiggers@kernel.org/

# perhaps curious, but you don't need any special privileges to enable fsverity on a file.

from contextlib import contextmanager
import os
import os.path
import string

block_size = 4096
digest_size = 256 // 8  # sha256
hashes_per_block = 4096 // (256 // 8)

VALID_CHARS = frozenset(string.ascii_lowercase + '_-' + string.digits)

@contextmanager
def testfile(id):
    assert all(c in VALID_CHARS for c in id)
    os.makedirs('testfiles', exist_ok=True)
    fn = 'testfiles/{}'.format(id)
    if os.path.exists(fn): os.unlink(fn)
    f = open(fn, 'wb')
    print("writing {}...".format(fn))
    yield f
    f.close()

with testfile('empty') as f:
    f.write(b'')

with testfile('onebyte') as f:
    f.write(b'A')

with testfile('oneblock') as f:
    f.write(b'A' * block_size)

with testfile('oneblockplusonebyte') as f:
    f.write(b'A' * block_size + b'B')

for i in [-1, 0, 1]:
    for j in [-1, 0, 1]:
        with testfile('hashblock_{}_{}'.format(i, j)) as f:
            f.write(b'A' * (block_size * (hashes_per_block + i) + j))

with testfile('longfile') as f:
    size = hashes_per_block * hashes_per_block * block_size * 3 + 99
    data = b''
    while len(data) < size:
        print("size = {} / {}".format(len(data), size))
        data += ''.join('{:08x}'.format(len(data) + i) for i in range(0, 1024 * 1024 * 5, 8)).encode('ascii')
    data = data[:size]
    f.write(data)


# show results using: fsverity measure testfiles/*
# output:
# sha256:3d248ca542a24fc62d1c43b916eae5016878e2533c88238480b26128a1f1af95 testfiles/empty
# sha256:21ac11f1c7319e1561beb9328375f73c142f7c38c0130f9b799b5290afa051ef testfiles/morelevels
# sha256:3fd7a78101899a79cd337b1b4e5414be8bcb376b133370156ef6e65026d930ed testfiles/oneblock
# sha256:c0b9455d545b6b1ee5e7b227bd1ed463aaa530a4840dcd93465163a2b3aff0da testfiles/oneblockplusonebyte
# sha256:9845e616f7d2f7a1cd6742f0546a36d2e74d4eb8ae7d9bdc0b0df982c27861b7 testfiles/onebyte