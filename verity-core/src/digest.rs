use digest::core_api::BlockSizeUser;
use digest::{FixedOutputReset, HashMarker, OutputSizeUser};
use sha2::digest::generic_array::typenum::Unsigned;
use sha2::digest::{self, Digest, FixedOutput, Reset, Update};
use sha2::{Sha256, Sha512};
use std::{io::Write, mem};

use crate::config::{
    InnerHashAlgorithm, DEFAULT_BLOCK_SIZE, MAX_DIGEST_SIZE, MAX_LEVELS, MAX_SALT_SIZE,
};
use crate::descriptor::FsVerityDescriptor;
use crate::tree::MerkleTree;

static ZEROES: [u8; 128] = [0u8; 128]; // sort of arbitrary power of two >= hash input block sizes

/// Trait for the inner hash algorithms we support (currently implemented for [`Sha256`] and [`Sha512`]).
///
/// It adds some information we need, some useful functions, and declares all the trait bounds we need
/// so we have them in one place.
pub trait InnerHash: Digest + BlockSizeUser + Clone + Default {
    /// The value of [`InnerHashAlgorithm`] that corresponds to this hash algorithm.
    const INNER_HASH_ALGORITHM: InnerHashAlgorithm;

    /// Update the hash state with given data, padded with zero bytes to the given size.
    /// This will panic if `data.len() > padded_size`.
    fn update_padded(&mut self, data: &[u8], padded_size: usize) {
        self.update(data);
        self.update_zeroes(padded_size.checked_sub(data.len()).unwrap());
    }

    /// Update the hash state with the given amount of zero bytes
    fn update_zeroes(&mut self, amount: usize) {
        let (quotient, remainder) = (amount / ZEROES.len(), amount % ZEROES.len());
        if remainder != 0 {
            self.update(&ZEROES[..remainder]);
        }
        for _ in 0..quotient {
            self.update(ZEROES);
        }
    }
}

impl InnerHash for Sha256 {
    const INNER_HASH_ALGORITHM: InnerHashAlgorithm = InnerHashAlgorithm::Sha256;
}

impl InnerHash for Sha512 {
    const INNER_HASH_ALGORITHM: InnerHashAlgorithm = InnerHashAlgorithm::Sha512;
}

/// Logically this represents a fixed-size block of data to be hashed (padded with zeroes if needed.)
/// It actually remembers only the hash state and how many more bytes are needed, not the data itself.
/// But that's an implementation detail.
#[derive(Clone)]
struct FixedSizeBlock<D>
where
    D: InnerHash,
{
    inner: D,
    remaining: usize,
}

impl<D> FixedSizeBlock<D>
where
    D: InnerHash,
{
    fn new<S: AsRef<[u8]> + Clone + Default>(config: &FsVerityConfig<D, S>) -> Self {
        Self {
            inner: config.salted_digest(),
            remaining: config.block_size,
        }
    }

    /// Appends data to block, panics if it doesn't fit.
    fn append(&mut self, data: &[u8]) {
        self.inner.update(data);
        self.remaining = self.remaining.checked_sub(data.len()).unwrap();
    }

    /// Fills the remaining space in the block with zero bytes.
    fn fill_to_end(&mut self) {
        self.inner.update_zeroes(self.remaining);
        self.remaining = 0;
    }

    /// Appends as much as possible to the block, returning the data that wouldn't fit.
    fn overflowing_append<'a>(&mut self, data: &'a [u8]) -> &'a [u8] {
        let (a, b) = data.split_at(self.remaining.min(data.len()));
        self.append(a);
        b
    }

    // Returns the final hash of the block, consuming it.
    fn finalize_into(mut self, dest: &mut digest::Output<D>) {
        self.fill_to_end();
        self.inner.finalize_into(dest);
    }

    /// Return the final hash of the block, and then reset its state to a copy of the given block.
    fn finalize_into_and_reset<S: AsRef<[u8]> + Clone + Default>(
        &mut self,
        dest: &mut digest::Output<D>,
        config: &FsVerityConfig<D, S>,
    ) {
        self.fill_to_end();

        let Self { inner, .. } = mem::replace(self, Self::new(config));

        inner.finalize_into(dest);
    }
}

/// Split out so we can pass it to FixedBlock::finalize_into_and_reset while self.levels is borrowed mutably.
#[derive(Clone)]
struct FsVerityConfig<D = Sha256, S = [u8; 0]>
where
    D: InnerHash,
    S: AsRef<[u8]> + Clone + Default,
{
    block_size: usize,
    /// We have to keep the actual salt around (not just its digest) as it is needed for the final hash operation.
    salt: S,
    salted_digest: D,
}

impl<D, S> FsVerityConfig<D, S>
where
    D: InnerHash,
    S: AsRef<[u8]> + Clone + Default,
{
    fn new(block_size: usize, salt: S) -> Self {
        // TODO error instead of panic?
        assert!(salt.as_ref().len() <= MAX_SALT_SIZE);
        assert!(block_size.is_power_of_two());
        assert!(block_size >= D::OutputSize::USIZE * 2);
        assert!(D::OutputSize::USIZE <= MAX_DIGEST_SIZE);

        let salted_digest = salt_to_digest(&salt);

        Self {
            block_size,
            salt,
            salted_digest,
        }
    }

    /// Returns an instance of the hash algorithm which has been fed the given salt,
    /// zero-padded to a multiple of the hash algorithm's input block size.
    fn salted_digest(&self) -> D {
        self.salted_digest.clone()
    }

    fn inner_hash_algorithm(&self) -> InnerHashAlgorithm {
        D::INNER_HASH_ALGORITHM
    }
}

pub fn salt_to_digest<D, S>(salt: &S) -> D
where
    D: InnerHash,
    S: AsRef<[u8]> + Clone + Default,
{
    let mut salted_digest = <D as digest::Digest>::new();
    // in practice this will run either 0 or 1 iterations, due to low MAX_SALT_SIZE
    for chunk in salt.as_ref().chunks(D::BlockSize::USIZE) {
        salted_digest.update_padded(chunk, D::BlockSize::USIZE);
    }
    salted_digest
}

/// Calculates an fs-verity measurement over the input data.
#[derive(Clone)]
pub struct FsVerityDigest<D = Sha256, S = [u8; 0]>
where
    D: InnerHash,
    S: AsRef<[u8]> + Clone + Default,
{
    /// The parameters of the verity hash
    config: FsVerityConfig<D, S>,
    /// The currently relevant hierarchy of blocks in the Merkle tree.
    levels: Vec<FixedSizeBlock<D>>,
    /// The Merkle tree layers, if record is requested.
    merkle_tree: MerkleTree<D>,
}

impl<D> FsVerityDigest<D>
where
    D: InnerHash,
{
    /// Creates a new instance of `FsVerityDigest` with an empty salt.
    ///
    /// This is probably what you want.
    pub fn new() -> Self {
        Self::new_with_salt(Default::default())
    }
}

impl<D, S> FsVerityDigest<D, S>
where
    D: InnerHash,
    S: AsRef<[u8]> + Clone + Default,
{
    /// Creates a new instance of `FsVerityDigest` with the given salt. The salt will be mixed
    /// into every internal hash calculation.
    ///
    /// Note that the current Linux kernel does not allow you to read back the salt used for a
    /// particular verity-protected file, which may be a problem depending on your use case.
    ///
    /// This will panic if the salt is longer than [`MAX_SALT_SIZE`] bytes.
    pub fn new_with_salt(salt: S) -> Self {
        Self::new_with_salt_and_block_size(salt, DEFAULT_BLOCK_SIZE)
    }

    /// Creates a new instance of `FsVerityDigest` with the given salt and a custom block size.
    ///
    /// This will panic if the salt is longer than [`MAX_SALT_SIZE`] bytes.
    ///
    /// If you want to be compatible with the Linux kernel implementation it is *not* a good idea
    /// to change the `block_size`, as the kernel currently requires it to be equal to the system
    /// page size, which is 4096 on most architectures. Some modern 64 bit ARM systems [have a
    /// 64kB page size](https://www.kernel.org/doc/Documentation/arm64/memory.txt) though.
    ///
    /// The block size must be a power of two, and it must be at least twice the size of the
    /// digests produced by the inner hash algorithm. This code will panic otherwise.
    pub fn new_with_salt_and_block_size(salt: S, block_size: usize) -> Self {
        let config = FsVerityConfig::new(block_size, salt);

        Self {
            config,
            levels: vec![],
            merkle_tree: Default::default(),
        }
    }

    /// Returns the [`InnerHashAlgorithm`] value corresponding to the used inner hash algorithm.
    pub fn inner_hash_algorithm(&self) -> InnerHashAlgorithm {
        self.config.inner_hash_algorithm()
    }
}

impl<D, S> Default for FsVerityDigest<D, S>
where
    D: InnerHash,
    S: AsRef<[u8]> + Clone + Default,
{
    fn default() -> Self {
        Self::new_with_salt(Default::default())
    }
}

impl<D, S> Update for FsVerityDigest<D, S>
where
    D: InnerHash,
    S: AsRef<[u8]> + Clone + Default,
{
    fn update(&mut self, data: &[u8]) {
        // self.levels represents the hierarchy of currently-relevant Merkle tree blocks.
        // level 0 is filled with input data. when the block at level n fills up, the hash of its
        // contents is appended to the block at level n + 1, and it is reset to an empty state.
        // this process can repeat if that causes the next level to fill up and so on.
        //
        // we do not actually keep the data written into each block, only the hash state.
        //
        // invariants:
        // - level 0 is (once it's created) never empty. it *may* be completely full.
        // - levels 1..n are never full, they always have room for one more hash. they *may* be empty.
        // - overflow is never larger than self.block_size
        //
        // the reason for the asymmetry between flushing of level[0] and the others is that it makes
        // flushing the final state (at the end of file) a lot simpler, because it guarantees that
        // each level will produce exactly one more digest for the next level during the final flush.
        // things could be a bit simpler if we always got input data in multiples of block_size and
        // if we knew the amount of data ahead of time so we could special-case the last block.
        for chunk in data.as_ref().chunks(self.config.block_size) {
            // keep moving up the hierarchy as long as dealing with the current level produces data
            // that needs to be appended to the next level.
            let mut keep_space_for_one_digest = false;
            let mut last_digest: digest::Output<Self>;
            let mut overflow = chunk; // input data is treated as overflow into level[0]
            for (level_index, level) in self.levels.iter_mut().enumerate() {
                // due to multiple reasons, the overflowing_append call will only ever split the data
                // in overflow across two blocks when writing input data (into level 0.) splitting a
                // digest across two blocks would be incorrect, so it is good that this never happens.
                // (the first reason is that valid block sizes and currently known digest sizes are all
                // powers of two, so the block size is always an exact multiple of the digest size.
                // the second reason is that (as mentioned) we always make sure there is room for one
                // more full digest.)
                overflow = level.overflowing_append(overflow);
                if keep_space_for_one_digest {
                    // done if there is enough space left in this level for one full digest
                    if level.remaining >= D::OutputSize::USIZE {
                        assert!(overflow.is_empty()); // if remaining > 0 there can't be overflow
                        break;
                    }
                } else {
                    // done if there was no overflow, even if the block is now totally full
                    if overflow.is_empty() {
                        break;
                    }
                }

                // can't write directly into last_digest because overflow is (sometimes) a
                // reference to last_digest, so we have to wait until we're done with overflow.
                let mut tmp: digest::Output<Self> = Default::default();
                level.finalize_into_and_reset(&mut tmp, &self.config);
                level.append(overflow);
                if level_index == 0 {
                    self.merkle_tree.append_level_1_hash(tmp.clone());
                }
                last_digest = tmp;

                overflow = &last_digest;

                keep_space_for_one_digest = true; // only false for level[0]
            }

            // if there is still overflow, add a new top level to the Merkle tree
            if !overflow.is_empty() {
                // note that this is very unlikely, with default settings you will have to write
                // more bytes than can be counted with an u64 before hitting this.
                assert!(self.levels.len() < MAX_LEVELS);

                let mut level = FixedSizeBlock::new(&self.config);
                level.append(overflow);
                self.levels.push(level);
            }
        }
    }
}

impl<D, S> OutputSizeUser for FsVerityDigest<D, S>
where
    D: InnerHash,
    S: AsRef<[u8]> + Clone + Default,
{
    type OutputSize = D::OutputSize;
}

impl<D, S> FsVerityDigest<D, S>
where
    D: InnerHash,
    S: AsRef<[u8]> + Clone + Default,
{
    pub fn finalize_into_fs_verity_stuffs(mut self) -> (FsVerityDescriptor<D>, MerkleTree<D>) {
        // a block is block_size bytes; a digest is D::OutputSize::USIZE bytes.
        // dividing them gives the "compression factor", meaning how many times smaller the
        // representation of the data becomes each time it moves up a level in the tree.
        // this means that for every level we go up, each byte in that level represents
        // this many times more input bytes than the previous level.
        // we will use this to recover the total number of bytes written from the number
        // of bytes written to each level. this is needed to calculate the final digest.
        // we could also have tracked the number of bytes written directly of course, but
        // besides showing off I guess this is a good consistency check.
        let compression_factor = self.config.block_size / D::OutputSize::USIZE;
        let mut total_size: usize = 0;
        let mut scale: usize = 1; // at level[0], each byte represents 1 input byte

        // flush all levels, and calculate the hash of the top level. in the docs for fs_verity
        // this is called the "root hash". zero length files are defined to have a root hash of
        // all zeroes, and we can comply with this easily by initializing last_digest to zero.
        // the root hash is ambiguous by itself, since it is simply a hash of block_size bytes
        // of data, and that data could have been either file data or digests of other blocks.
        // you always need the file size as well to properly interpret the root hash.
        // this means there is no additional risk of ambiguity if a block is ever discovered
        // which happens to hash to all zeros.
        let mut last_digest: digest::Output<Self> = Default::default();
        let mut overflow: &[u8] = &[];
        for (level_index, mut level) in self.levels.into_iter().enumerate() {
            total_size += scale * (self.config.block_size - level.remaining);
            level.append(overflow);
            level.finalize_into(&mut last_digest);
            if level_index == 0 {
                self.merkle_tree.append_level_1_hash(last_digest.clone());
            }
            overflow = &last_digest;
            scale *= compression_factor;
        }

        (
            FsVerityDescriptor {
                version: 1,
                hash_algorithm: self.config.inner_hash_algorithm() as u8,
                log_blocksize: self.config.block_size.trailing_zeros() as u8,
                data_size: total_size as u64,
                root_hash: last_digest,
                salt: self.config.salt.as_ref().to_owned(),
            },
            self.merkle_tree,
        )
    }
}

impl<D, S> FixedOutput for FsVerityDigest<D, S>
where
    D: InnerHash,
    S: AsRef<[u8]> + Clone + Default,
{
    fn finalize_into(self, out: &mut digest::Output<Self>) {
        let (descriptor, _) = self.finalize_into_fs_verity_stuffs();
        *out = descriptor.to_descriptor_hash();
    }
}

/// NOTE: This reports the base hash function's input block size, *not* the Merkle tree block size you
/// specify when creating the `FsVerityDigest`!
///
/// While this may be confusing, it is probably more faithful to the purpose of the `BlockInput` trait.
/// We don't need to buffer an entire Merkle tree block in memory; data can be efficiently processed
/// in chunks of the base hash's input block size.
impl<D, S> BlockSizeUser for FsVerityDigest<D, S>
where
    D: InnerHash,
    S: AsRef<[u8]> + Clone + Default,
{
    /// Equal to the inner hash algorithm's block size.
    type BlockSize = D::BlockSize;
}

/// Resets to a blank state, but with the same Merkle tree block size and salt
impl<D, S> Reset for FsVerityDigest<D, S>
where
    D: InnerHash,
    S: AsRef<[u8]> + Clone + Default,
{
    fn reset(&mut self) {
        // Pretty basic implementation but good enough
        *self =
            Self::new_with_salt_and_block_size(self.config.salt.clone(), self.config.block_size);
    }
}

impl<D, S> FixedOutputReset for FsVerityDigest<D, S>
where
    D: InnerHash,
    S: AsRef<[u8]> + Clone + Default,
{
    fn finalize_into_reset(&mut self, out: &mut digest::Output<Self>) {
        // Pretty basic implementation but good enough
        let new =
            Self::new_with_salt_and_block_size(self.config.salt.clone(), self.config.block_size);
        let old = mem::replace(self, new);
        FixedOutput::finalize_into(old, out);
    }
}

impl<D, S> Write for FsVerityDigest<D, S>
where
    D: InnerHash,
    S: AsRef<[u8]> + Clone + Default,
{
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Update::update(self, buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<D, S> HashMarker for FsVerityDigest<D, S>
where
    D: InnerHash,
    S: AsRef<[u8]> + Clone + Default,
{
}

/// Alias for `FsVerityDigest<Sha256>`
pub type FsVeritySha256<S> = FsVerityDigest<Sha256, S>;

/// Alias for `FsVerityDigest<Sha512>`
pub type FsVeritySha512<S> = FsVerityDigest<Sha512, S>;

/// For trait objects of [`FsVerityDigest`], when the inner hash is not statically known
pub trait DynFsVerityDigest: sha2::digest::DynDigest + Write {
    fn inner_hash_algorithm(&self) -> InnerHashAlgorithm;
}
impl<D: InnerHash + 'static, S: AsRef<[u8]> + Clone + Default + 'static> DynFsVerityDigest
    for FsVerityDigest<D, S>
{
    fn inner_hash_algorithm(&self) -> InnerHashAlgorithm {
        self.config.inner_hash_algorithm()
    }
}

/// Like [`FsVerityDigest::new`], but you can choose the hash algorithm at runtime.
pub fn new_dyn(inner_hash: InnerHashAlgorithm) -> Box<dyn DynFsVerityDigest> {
    match inner_hash {
        InnerHashAlgorithm::Sha256 => Box::new(FsVeritySha256::new()),
        InnerHashAlgorithm::Sha512 => Box::new(FsVeritySha512::new()),
    }
}

/// Like [`FsVerityDigest::new_with_salt`], but you can choose the hash algorithm at runtime.
///
/// Please check the linked function for additional notes about specifying a salt.
pub fn new_dyn_with_salt<S: AsRef<[u8]> + Clone + Default + 'static>(
    inner_hash: InnerHashAlgorithm,
    salt: S,
) -> Box<dyn DynFsVerityDigest> {
    match inner_hash {
        InnerHashAlgorithm::Sha256 => Box::new(FsVeritySha256::new_with_salt(salt)),
        InnerHashAlgorithm::Sha512 => Box::new(FsVeritySha512::new_with_salt(salt)),
    }
}

/// Like [`FsVerityDigest::new_with_salt_and_block_size`], but you can choose the hash algorithm at runtime.
///
/// Please check the linked function for additional notes about specifying a salt and block size.
pub fn new_dyn_with_salt_and_block_size<S: AsRef<[u8]> + Clone + Default + 'static>(
    inner_hash: InnerHashAlgorithm,
    salt: S,
    block_size: usize,
) -> Box<dyn DynFsVerityDigest> {
    match inner_hash {
        InnerHashAlgorithm::Sha256 => Box::new(FsVeritySha256::new_with_salt_and_block_size(
            salt, block_size,
        )),
        InnerHashAlgorithm::Sha512 => Box::new(FsVeritySha512::new_with_salt_and_block_size(
            salt, block_size,
        )),
    }
}

#[cfg(test)]
mod tests {
    use crate::digest::{FsVeritySha256, InnerHashAlgorithm};
    use std::fs::File;
    use std::io::{BufRead as _, BufReader, Read as _};
    use std::path::PathBuf;

    use super::new_dyn;

    fn get_testfiles() -> Vec<(InnerHashAlgorithm, Vec<u8>, PathBuf)> {
        // 'longfile' takes a while in debug mode, about 20 seconds for me.
        // in release mode it takes about a second.
        // sha256:e228078ebe9c4f7fe0c5d6a76fb2e317f5ea8bdfb227d7741e5c57cff739b5fa testfiles/longfile
        let testfiles = "
        sha256:3d248ca542a24fc62d1c43b916eae5016878e2533c88238480b26128a1f1af95 testfiles/empty
        sha256:f5c2b9ded1595acfe8a996795264d488dd6140531f6a01f8f8086a83fd835935 testfiles/hashblock_0_0
        sha256:5c00a54bd1d8341d7bbad060ff1b8e88ed2646d7bb38db6e752cd1cff66c0a78 testfiles/hashblock_0_-1
        sha256:a7abb76568871169a79104d00679fae6521dfdb2a2648e380c02b10e96e217ff testfiles/hashblock_0_1
        sha256:c4b519068d8c8c68fd5e362fc3526c5b11e15f8eb72d4678017906f9e7f2d137 testfiles/hashblock_-1_0
        sha256:09510d2dbb55fa16f2768165c42d19c4da43301dfaa05705b2ecb4aaa4a5686a testfiles/hashblock_1_0
        sha256:7aa0bb537c623562f898386ac88acd319267e4ab3200f3fd1cf648cfdb4a0379 testfiles/hashblock_-1_-1
        sha256:f804e9777f91d3697ca015303c23251ad3d80205184cfa3d1066ab28cb906330 testfiles/hashblock_-1_1
        sha256:26159b4fc68c63881c25c33b23f2583ffaa64fee411af33c3b03238eea56755c testfiles/hashblock_1_-1
        sha256:57bed0934bf3ab4610d54938f03cff27bd0d9d76c9a77e283f9fb2b7e29c5ab8 testfiles/hashblock_1_1
        sha256:3fd7a78101899a79cd337b1b4e5414be8bcb376b133370156ef6e65026d930ed testfiles/oneblock
        sha256:c0b9455d545b6b1ee5e7b227bd1ed463aaa530a4840dcd93465163a2b3aff0da testfiles/oneblockplusonebyte
        sha256:9845e616f7d2f7a1cd6742f0546a36d2e74d4eb8ae7d9bdc0b0df982c27861b7 testfiles/onebyte
        ".trim().lines().map(|l| {
            let l = l.trim();
            let (digest, path) = l.split_once(" ").unwrap();
            let (digest_type, digest) = digest.split_once(":").unwrap();
            let digest_type = digest_type.parse::<super::InnerHashAlgorithm>().unwrap();
            let digest = hex::decode(digest).unwrap();
            (digest_type, digest, PathBuf::from(path))
        }).collect::<Vec<_>>();
        testfiles
    }

    #[test]
    fn test_testfiles() {
        let testfiles = get_testfiles();

        for (digest_type, digest, path) in testfiles {
            assert!(digest_type == InnerHashAlgorithm::Sha256);

            {
                let mut f = BufReader::new(File::open(&path).unwrap());
                let mut tmp = new_dyn(digest_type);
                std::io::copy(&mut f, &mut tmp).unwrap();
                let out = tmp.finalize();

                assert!(
                    out.as_ref() == digest,
                    "expected: {} found: {} for file: {:?}",
                    hex::encode(&digest),
                    hex::encode(&out),
                    path
                );
            }

            {
                let mut f = BufReader::new(File::open(&path).unwrap());
                let mut tmp = FsVeritySha256::new();
                std::io::copy(&mut f, &mut tmp).unwrap();
                let (descriptor, merkle_tree) = tmp.finalize_into_fs_verity_stuffs();

                // Check descriptor hash
                let out = descriptor.to_descriptor_hash();
                assert!(
                    out.to_vec() == digest,
                    "expected: {} found: {} for file: {:?}",
                    hex::encode(&digest),
                    hex::encode(out),
                    path
                );

                // Check merkle tree root hash
                let rebuild_root_hash =
                    merkle_tree.rebuild_root_hash(descriptor.salt.clone(), descriptor.block_size());
                assert!(
                    rebuild_root_hash == descriptor.root_hash,
                    "expected(root_hash): {} found: {} for file: {:?}",
                    hex::encode(descriptor.root_hash),
                    hex::encode(rebuild_root_hash),
                    path
                );

                // Check each data block

                let block_size = descriptor.block_size();
                let mut reader = BufReader::new(File::open(path).expect("Failed to open file"));
                let mut buffer = vec![0u8; block_size];
                let mut block_index = 0;

                loop {
                    match reader.read_exact(&mut buffer) {
                        Ok(()) => {
                            assert!(
                                merkle_tree.verify_data_block(block_index, block_size, &buffer),
                                "Block verification failed: index={}",
                                block_index
                            );
                            block_index += 1;
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                            let rem = reader.fill_buf().unwrap();
                            if !rem.is_empty() {
                                assert!(
                                    merkle_tree.verify_data_block(block_index, block_size, rem),
                                    "Final block verification failed: index={}, len={}",
                                    block_index,
                                    rem.len()
                                );
                            }
                            break;
                        }
                        Err(e) => panic!("Failed to read file: {}", e),
                    }
                }
            }
        }
    }
}
