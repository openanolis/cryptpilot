use anyhow::{Context as _, Result};
use devicemapper::{DevId, DmFlags, DmName, DmOptions, DM};
use rand::{distributions::Alphanumeric, Rng as _};

struct DeviceMapperDevice {
    dm: DM,
    device_name: String,
}

#[allow(unused)]
impl DeviceMapperDevice {
    pub async fn new_zero(device_size: u64) -> Result<Self> {
        // Generate a random name
        let random_part: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(16) // Fixed length characters
            .map(char::from)
            .collect();
        let device_name = format!("cryptpilot-{}", random_part);

        let dm = DM::new().context("Failed to communicating with device-mapper driver")?;
        let dm_name = DmName::new(&device_name)
            .with_context(|| format!("{device_name} is not a valid device name"))?;
        let _dev = dm
            .device_create(dm_name, None, DmOptions::default())
            .context("Failed to create device-mapper device")?;

        let dm_id = DevId::Name(dm_name);
        let table = vec![(0, device_size, "zero".into(), "".into())];

        dm.table_load(
            &dm_id,
            &table,
            DmOptions::default().set_flags(DmFlags::DM_PERSISTENT_DEV),
        )
        .context("Failed to load device-mapper table")?;

        dm.device_suspend(&dm_id, DmOptions::default())
            .context("Failed to resume device-mapper device")?;

        Ok(DeviceMapperDevice { dm, device_name })
    }

    pub fn path(&self) -> String {
        format!("/dev/mapper/{}", self.device_name)
    }
}

impl Drop for DeviceMapperDevice {
    fn drop(&mut self) {
        let dm_name = match DmName::new(&self.device_name)
            .with_context(|| format!("{} is not a valid device name", self.device_name))
        {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("{e:#}");
                return;
            }
        };

        let dm_id = DevId::Name(dm_name);
        if let Err(e) = self
            .dm
            .device_remove(&dm_id, DmOptions::default())
            .context("Failed to remove device-mapper device")
        {
            tracing::error!("{e:#}")
        };
    }
}
