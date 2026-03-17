pub mod after_sysroot;
pub mod before_sysroot;

// The final rootfs device - Used for mounting as root filesystem
pub const ROOTFS_NAME: &str = "rootfs";
pub const ROOTFS_DEVICE: &str = "/dev/mapper/rootfs";

// Rootfs logical volume in LVM
pub const ROOTFS_LOGICAL_VOLUME: &str = "/dev/mapper/system-rootfs";
// Rootfs decrypted which will be used as backend for dm-verity
pub const ROOTFS_DECRYPTED_NAME: &str = "rootfs_decrypted";
pub const ROOTFS_DECRYPTED_LAYER_DEVICE: &str = "/dev/mapper/rootfs_decrypted";
// Rootfs dm-verity hash device
pub const ROOTFS_HASH_LOGICAL_VOLUME: &str = "/dev/mapper/system-rootfs_hash";

// The final data partition device - LUKS2 encrypted device for data partition
pub const DATA_NAME: &str = "data";
pub const DATA_DEVICE: &str = "/dev/mapper/data";
// Data logical volume in LVM
pub const DATA_LOGICAL_VOLUME: &str = "/dev/mapper/system-data";

// dm-snapshot related constants
// dm-verity device name for dm-snapshot backend
pub const ROOTFS_VERITY_NAME: &str = "rootfs_verity";
pub const ROOTFS_VERITY_DEVICE: &str = "/dev/mapper/rootfs_verity";
// dm-linear device combining dm-verity and zero target (extended rootfs for snapshot)
pub const ROOTFS_EXTENDED_NAME: &str = "rootfs_extended";
pub const ROOTFS_EXTENDED_DEVICE: &str = "/dev/mapper/rootfs_extended";
