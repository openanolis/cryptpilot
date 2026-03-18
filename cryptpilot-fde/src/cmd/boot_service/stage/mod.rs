pub mod after_sysroot;
pub mod before_sysroot;

// LVM related constants
// Volume group name in LVM
pub const VOLUME_GROUP_NAME: &str = "cryptpilot";
// Rootfs logical volume in LVM
pub const ROOTFS_LOGICAL_VOLUME: &str = "/dev/mapper/cryptpilot-rootfs";
// Rootfs dm-verity hash device
pub const ROOTFS_HASH_LOGICAL_VOLUME: &str = "/dev/mapper/cryptpilot-rootfs_hash";
// Delta logical volume in LVM
pub const DELTA_LOGICAL_VOLUME: &str = "/dev/mapper/cryptpilot-delta";

// The final rootfs device - Used for mounting as root filesystem
pub const ROOTFS_NAME: &str = "rootfs";
pub const ROOTFS_DEVICE: &str = "/dev/mapper/rootfs";

// Rootfs decrypted which will be used as backend for dm-verity
pub const ROOTFS_DECRYPTED_NAME: &str = "rootfs_decrypted";
pub const ROOTFS_DECRYPTED_LAYER_DEVICE: &str = "/dev/mapper/rootfs_decrypted";

// The final delta partition device - LUKS2 encrypted device for delta partition
pub const DELTA_NAME: &str = "delta";
pub const DELTA_DEVICE: &str = "/dev/mapper/delta";

// dm-snapshot related constants
// dm-verity device name for dm-snapshot backend
pub const ROOTFS_VERITY_NAME: &str = "rootfs_verity";
pub const ROOTFS_VERITY_DEVICE: &str = "/dev/mapper/rootfs_verity";
// dm-linear device combining dm-verity and zero target (extended rootfs for snapshot)
pub const ROOTFS_EXTENDED_NAME: &str = "rootfs_extended";
pub const ROOTFS_EXTENDED_DEVICE: &str = "/dev/mapper/rootfs_extended";
