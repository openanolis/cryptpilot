pub mod after_sysroot;
pub mod auto_open;
pub mod before_sysroot;

const ROOTFS_LOGICAL_VOLUME: &str = "/dev/mapper/system-rootfs";
const ROOTFS_LAYER_NAME: &str = "rootfs";
const ROOTFS_LAYER_DEVICE: &str = "/dev/mapper/rootfs";
const ROOTFS_DECRYPTED_LAYER_DEVICE: &str = "/dev/mapper/rootfs_decrypted";
const ROOTFS_DECRYPTED_LAYER_NAME: &str = "rootfs_decrypted";
const ROOTFS_HASH_LOGICAL_VOLUME: &str = "/dev/mapper/system-rootfs_hash";
const DATA_LOGICAL_VOLUME: &str = "/dev/mapper/system-data";
const DATA_LAYER_NAME: &str = "data";
const DATA_LAYER_DEVICE: &str = "/dev/mapper/data";
