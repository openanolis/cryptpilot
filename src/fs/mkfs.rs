use anyhow::{bail, Result};

use crate::config::volume::MakeFsType;

use super::shell::Shell;

impl MakeFsType {
    pub fn to_systemd_makefs_fstype(&self) -> &'static str {
        match self {
            MakeFsType::Swap => "swap",
            MakeFsType::Ext4 => "ext4",
            MakeFsType::Xfs => "xfs",
            MakeFsType::Vfat => "vfat",
        }
    }

    pub fn mkfs_on_no_wipe_volume_blocking(&self, volume_path: &str) -> Result<()> {
        let script = match self {
            MakeFsType::Swap => {
                format!(
                    r#"
                    dd if=/dev/zero of={volume_path} count=1 seek=0 bs=4096
                    mkswap {volume_path}
                    "#,
                )
            }
            MakeFsType::Ext4 => {
                format!(
                    r#"
                    BLOCKS=$(mkfs.ext4 -n {volume_path} | tail -n 4 | grep -Eo '[0-9]{{4,}}' | sort -n)
                    BLOCKS="0 $BLOCKS"

                    for BLOCK_NUM in $BLOCKS
                    do
                        dd if=/dev/zero of={volume_path} count=1 seek=$BLOCK_NUM bs=4096
                        dd if={volume_path}  count=1 skip=$BLOCK_NUM  bs=4096 | hexdump 
                    done
                    mkfs.ext4 {volume_path}
                    "#,
                )
            }
            MakeFsType::Xfs => {
                format!(
                    r#"
                    dd if=/dev/zero of={volume_path} count=1 seek=0 bs=4096
                    mkfs.xfs -f {volume_path}
                    "#,
                )
            }
            MakeFsType::Vfat => {
                bail!("The option `makefs=vfat` and `integrity=true` is not currently supported")
            }
        };
        Shell(script).run()
    }
}
