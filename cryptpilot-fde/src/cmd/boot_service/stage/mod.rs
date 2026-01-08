pub mod after_sysroot;
pub mod before_sysroot;

// Rootfs encryption layer - LUKS2 encrypted device for root filesystem
pub const ROOTFS_LAYER_NAME: &str = "rootfs";
pub const ROOTFS_LAYER_DEVICE: &str = "/dev/mapper/rootfs";
// Rootfs logical volume in LVM
pub const ROOTFS_LOGICAL_VOLUME: &str = "/dev/mapper/system-rootfs";
// Rootfs decrypted layer with dm-verity for integrity verification
pub const ROOTFS_DECRYPTED_LAYER_NAME: &str = "rootfs_decrypted";
pub const ROOTFS_DECRYPTED_LAYER_DEVICE: &str = "/dev/mapper/rootfs_decrypted";
// Rootfs dm-verity hash device
pub const ROOTFS_HASH_LOGICAL_VOLUME: &str = "/dev/mapper/system-rootfs--verity";

// Data partition encryption layer - LUKS2 encrypted device for data partition
pub const DATA_LAYER_NAME: &str = "data";
pub const DATA_LAYER_DEVICE: &str = "/dev/mapper/data";
// Data logical volume in LVM
pub const DATA_LOGICAL_VOLUME: &str = "/dev/mapper/system-data";
